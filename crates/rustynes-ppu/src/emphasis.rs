// SPDX-License-Identifier: GPL-3.0-or-later
//! PPUMASK colour emphasis, from the documented composite model (v2.9.8
//! "Vanguard", `T-EMPHASIS-MODEL`).
//!
//! # What the hardware does
//!
//! Emphasis is not an RGB operation. The 2C02 draws a composite waveform
//! directly, and PPUMASK bits 5-7 switch in ONE attenuator shared by all three
//! bits. Each bit arms it during the six colour-generator phases of one colour
//! square wave, so it "is active for 6 out of 12 half-clocks if one bit is set,
//! 10 half-clocks if two bits are set, or all 12 if all three bits are set", and
//! it never touches the blacks in columns `$E`/`$F`
//! (`nesdev_wiki/NTSC_video.xhtml`, "Color Tint Bits"). A picture tints toward
//! the COMPLEMENT of the attenuated phases, and with all three bits set the
//! whole signal drops together, so the result is darker but not tinted.
//!
//! # The model
//!
//! Written from that page's prose and data, not from any emulator and not from
//! [`crate::generate_base_palette`] (whose `// Provenance:` header discloses a
//! derived region):
//!
//! 1. **Levels.** The page's terminated measurements (lidnariq), in volts:
//!    the low level of each row (`$0D`, `$1D`, `$2D`, `$3D`), the high level
//!    (`$00`, `$10`, `$20`, `$30`), and both again attenuated (`$xDem`,
//!    `$x0em`). `$x1-$xC` alternate between the row's two levels; `$x0` holds
//!    the high one, `$xD` the low one, and `$xE`/`$xF` output `$1D`'s voltage.
//! 2. **Phases.** Colour `$xn` (`n` 1-12) is high on the six phases `p` of the
//!    twelve where `(p + n) mod 12` is 1 to 6 -- the "Color Phases" diagram,
//!    row by row (`[test] the_phase_rule_is_the_documented_diagram`).
//!    Emphasis bits 5, 6 and 7 arm the attenuator on the phases of colours
//!    `$C`, `$4` and `$8` (the "Color Tint Bits" table).
//! 3. **Decode.** One full colour cycle, twelve samples, each taken mid-phase.
//!    The signal is normalised between `$1D` (black; the blanking level is
//!    `$0F`'s, which is `$1D`'s voltage) and `$20` (white). `Y` is the mean;
//!    `U` and `V` are twice the mean of the signal times the reference carrier
//!    ("Chroma saturation correction": the integral of `sin^2` over a cycle is
//!    one half). The reference is aligned so the colour burst, which "is the
//!    same phase as phase 8", decodes as pure `-U`.
//! 4. **RGB.** The page's inverse matrix:
//!    `R = Y + 1.139883 V`, `G = Y - 0.394642 U - 0.580622 V`,
//!    `B = Y + 2.032062 U`.
//!
//! # How it meets the FBX palette
//!
//! The default palette is FBX "Smooth", a calibrated measurement, and it
//! stays the base. The model supplies only the CHANGE emphasis makes: for each
//! of the 64 colours and 7 non-zero emphasis settings, the model's emphasised
//! RGB minus its own un-emphasised RGB, in 8-bit units. [`EMPHASIS_DELTA`]
//! holds those 512 deltas and the renderer adds them to whatever base is
//! active, clamping to `0..=255`. A difference rather than a ratio, because
//! the model's darkest colours decode at or below black, where a ratio has no
//! meaning; with emphasis off every delta is exactly zero, so an un-emphasised
//! frame is byte-identical to what it was before this model existed.
//!
//! The table is data because `build_rgba_lut` is a `const fn` and the decode
//! needs `sin`/`cos`. [`documented_delta`] is the model itself, and a test
//! recomputes all 512 entries from it and compares them with the table, so the
//! two cannot drift. The `MiSTer` core ships the same resulting colours, checked
//! entry by entry by its `palette-gate`.
//!
//! Not modelled, by choice: the page's differential phase distortion, colour
//! artifacts between pixels, and the PAL/Dendy swap of the red and green bits.

/// The page's terminated levels in volts: `[plain, attenuated][low, high][row]`.
const LEVELS: [[[f64; 4]; 2]; 2] = [
    [[0.228, 0.312, 0.552, 0.880], [0.616, 0.840, 1.100, 1.100]],
    [[0.192, 0.256, 0.448, 0.712], [0.500, 0.676, 0.896, 0.896]],
];

/// Black (`$1D`, which `$0F` and blanking share) and white (`$20`), in volts.
const BLACK: f64 = 0.312;
const WHITE: f64 = 1.100;

#[cfg_attr(not(test), allow(dead_code))]
/// Whether colour `hue` (1-12) is high on phase `p` (0-11). The diagram:
/// colour 1 is high on phases 0-5, colour 2 on 0-4 and 11, and so on round.
#[must_use]
const fn phase_high(hue: u8, p: u8) -> bool {
    let k = (p + hue) % 12;
    k >= 1 && k <= 6
}

#[cfg_attr(not(test), allow(dead_code))]
/// The composite level of `colour` on phase `p` with emphasis `emph`
/// (bit 0 red / PPUMASK 5, bit 1 green / 6, bit 2 blue / 7).
fn level(colour: u8, emph: u8, p: u8) -> f64 {
    let hue = colour & 0x0F;
    let row = usize::from((colour >> 4) & 0x03);
    // `$xE`/`$xF`: `$1D`'s voltage, and never attenuated.
    if hue >= 0x0E {
        return LEVELS[0][0][1];
    }
    let attenuated = (emph & 1 != 0 && phase_high(0x0C, p))
        || (emph & 2 != 0 && phase_high(0x04, p))
        || (emph & 4 != 0 && phase_high(0x08, p));
    let set = &LEVELS[usize::from(attenuated)];
    let (low, high) = (set[0][row], set[1][row]);
    match hue {
        0x00 => high,
        0x0D => low,
        _ if phase_high(hue, p) => high,
        _ => low,
    }
}

#[cfg_attr(not(test), allow(dead_code))]
/// The model's RGB for `colour` under `emph`, unclipped, 0.0 black to 1.0
/// white.
fn decode(colour: u8, emph: u8) -> [f64; 3] {
    let (mut y, mut u, mut v) = (0.0, 0.0, 0.0);
    for p in 0u8..12 {
        let s = (level(colour, emph, p) - BLACK) / (WHITE - BLACK);
        // Mid-phase sample; the burst's phase (colour 8, high on phases 5-10)
        // is centred on 8.0, and must decode as pure -U.
        let angle = core::f64::consts::PI * (f64::from(p) + 0.5 - 8.0) / 6.0;
        y += s;
        u -= s * libm::cos(angle);
        v += s * libm::sin(angle);
    }
    let (y, u, v) = (y / 12.0, 2.0 * u / 12.0, 2.0 * v / 12.0);
    [
        y + 1.139_883 * v,
        y - 0.394_642 * u - 0.580_622 * v,
        y + 2.032_062 * u,
    ]
}

/// The documented model's change to `colour` (0-63) under `emph` (0-7): its
/// emphasised RGB minus its plain RGB, in 8-bit units, rounded to nearest.
///
/// Only the tests call it: the renderer reads [`EMPHASIS_DELTA`], and this is
/// what keeps that table honest. Hence the allow outside `cfg(test)`, and the
/// decode helpers it calls share it.
#[cfg_attr(not(test), allow(dead_code))]
#[must_use]
pub fn documented_delta(colour: u8, emph: u8) -> [i16; 3] {
    let on = decode(colour & 0x3F, emph & 0x07);
    let off = decode(colour & 0x3F, 0);
    let d = |c: usize| libm::round(255.0 * (on[c] - off[c])) as i16;
    [d(0), d(1), d(2)]
}

/// Add an emphasis delta to a base colour, clamped to a byte.
#[must_use]
pub const fn apply_delta(base: u8, delta: i16) -> u8 {
    let v = base as i16 + delta;
    if v < 0 {
        0
    } else if v > 255 {
        255
    } else {
        // In 0..=255 here, so neither the sign nor any bit is lost.
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let byte = v as u8;
        byte
    }
}

/// [`documented_delta`] for every `(emphasis << 6) | colour`, as data.
/// Regenerate with `cargo test -p rustynes-ppu -- --ignored
/// print_emphasis_delta_table --nocapture`; the test
/// `the_committed_table_is_the_documented_model` fails if this drifts.
pub const EMPHASIS_DELTA: [[i16; 3]; 512] = include!("emphasis_delta_table.rs");

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use core::fmt::Write as _;
    use std::{println, string::String};

    /// The diagram in "Color Phases", transcribed row by row.
    #[test]
    fn the_phase_rule_is_the_documented_diagram() {
        const ROWS: [&str; 12] = [
            "111111------",
            "22222------2",
            "3333------33",
            "444------444",
            "55------5555",
            "6------66666",
            "------777777",
            "-----888888-",
            "----999999--",
            "---AAAAAA---",
            "--BBBBBB----",
            "-CCCCCC-----",
        ];
        for (i, row) in ROWS.iter().enumerate() {
            let hue = u8::try_from(i + 1).unwrap();
            for (p, ch) in row.bytes().enumerate() {
                let p = u8::try_from(p).unwrap();
                assert_eq!(phase_high(hue, p), ch != b'-', "colour {hue} phase {p}");
            }
        }
    }

    /// Hues decode where the page says they are: `$x6` is the red colour
    /// wave, `$xA` green and `$x2` blue ("Color Tint Bits" complements).
    #[test]
    fn the_decode_puts_the_hues_where_the_page_does() {
        let [r, g, b] = decode(0x16, 0);
        assert!(r > g && r > b, "$16 {r} {g} {b}");
        let [r, g, b] = decode(0x1A, 0);
        assert!(g > r && g > b, "$1A {r} {g} {b}");
        let [r, g, b] = decode(0x12, 0);
        assert!(b > r && b > g, "$12 {r} {g} {b}");
    }

    /// Columns `$E`/`$F` are never attenuated; no emphasis means no change.
    #[test]
    fn the_blacks_and_no_emphasis_are_untouched() {
        for colour in 0..64u8 {
            assert_eq!(documented_delta(colour, 0), [0, 0, 0]);
            if colour & 0x0F >= 0x0E {
                for emph in 1..8 {
                    assert_eq!(documented_delta(colour, emph), [0, 0, 0], "${colour:02X}");
                }
            }
        }
    }

    /// All three bits attenuate every phase: darker and NOT tinted, so a grey
    /// stays grey. Any one bit tints toward its complement: red emphasis
    /// darkens green and blue more than red.
    #[test]
    fn three_bits_darken_evenly_and_one_bit_tints() {
        for colour in [0x00u8, 0x10, 0x20, 0x30, 0x2D] {
            let [r, g, b] = documented_delta(colour, 7);
            assert!(r < 0, "${colour:02X} {r}");
            assert!(
                (r - g).abs() <= 1 && (g - b).abs() <= 1,
                "${colour:02X} {r} {g} {b}"
            );
            let [r, g, b] = documented_delta(colour, 1);
            assert!(r > g && r > b, "${colour:02X} red {r} {g} {b}");
            let [r, g, b] = documented_delta(colour, 2);
            assert!(g > r && g > b, "${colour:02X} green {r} {g} {b}");
            let [r, g, b] = documented_delta(colour, 4);
            assert!(b > r && b > g, "${colour:02X} blue {r} {g} {b}");
        }
    }

    #[test]
    fn the_committed_table_is_the_documented_model() {
        for i in 0..512u16 {
            let colour = u8::try_from(i & 0x3F).unwrap();
            let emph = u8::try_from(i >> 6).unwrap();
            assert_eq!(
                EMPHASIS_DELTA[usize::from(i)],
                documented_delta(colour, emph),
                "entry {i:#05x}: regenerate emphasis_delta_table.rs"
            );
        }
    }

    /// Prints the table for `emphasis_delta_table.rs`.
    #[test]
    #[ignore = "a generator, not a check"]
    fn print_emphasis_delta_table() {
        let mut out = String::from(
            "// SPDX-License-Identifier: GPL-3.0-or-later\n\
             // GENERATED by `emphasis::tests::print_emphasis_delta_table` from the\n\
             // documented model in emphasis.rs. Do not edit by hand.\n[\n",
        );
        for i in 0..512u16 {
            let [r, g, b] = documented_delta(
                u8::try_from(i & 0x3F).unwrap(),
                u8::try_from(i >> 6).unwrap(),
            );
            let _ = writeln!(out, "    [{r}, {g}, {b}],");
        }
        out.push(']');
        println!("{out}");
    }
}

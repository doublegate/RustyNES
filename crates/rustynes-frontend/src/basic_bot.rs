//! `BasicBot` (v1.8.9) — a save-state-anchored brute-force input search, in the
//! spirit of `BizHawk`'s Basic Bot.
//!
//! From the current emulator state it snapshots an anchor, then repeatedly restores
//! that anchor, plays a randomly-drawn input sequence, and scores the result by a
//! target memory value (1 or 2 bytes). The best-scoring sequence is kept. It runs
//! entirely on the deterministic core via [`Nes::snapshot`] / [`Nes::restore`] /
//! [`Nes::run_frame`] / [`Nes::peek`] and its own seeded PRNG, so the same anchor +
//! [`BotConfig`] always yields the same result — it never perturbs live output (the
//! caller restores the anchor afterwards) and adds no hidden non-determinism.

use rustynes_core::{Buttons, Nes};

/// What the bot optimizes for and how hard it searches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BotConfig {
    /// CPU address whose value the bot maximizes.
    pub target_addr: u16,
    /// Read the target as a little-endian 16-bit value (`addr`, `addr+1`).
    pub two_byte: bool,
    /// Frames played per attempt.
    pub frames: usize,
    /// Number of random sequences to try.
    pub attempts: usize,
    /// Buttons the search may press on player 1 each frame (drawn at random; an
    /// empty draw = no press). Diagonals are possible by listing e.g. `UP | RIGHT`.
    pub pool: Vec<Buttons>,
    /// PRNG seed — the same seed reproduces the same search exactly.
    pub seed: u64,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            target_addr: 0x0000,
            two_byte: false,
            frames: 60,
            attempts: 200,
            pool: vec![
                Buttons::empty(),
                Buttons::A,
                Buttons::B,
                Buttons::LEFT,
                Buttons::RIGHT,
                Buttons::UP,
                Buttons::DOWN,
            ],
            seed: 0x1234_5678,
        }
    }
}

/// The outcome of a [`search`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BotResult {
    /// Best target value reached.
    pub best_score: u32,
    /// The player-1 input sequence (one [`Buttons`] per frame) that reached it.
    pub best_inputs: Vec<Buttons>,
    /// How many attempts actually ran.
    pub attempts_run: usize,
}

/// A small, fast, fully-deterministic PRNG (`SplitMix64`) — the bot's own
/// randomness so a search is reproducible without touching the core's RNG.
struct SplitMix64(u64);

impl SplitMix64 {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }
    const fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Read the configured target value (1 or 2 bytes, little-endian).
fn read_target(nes: &mut Nes, cfg: &BotConfig) -> u32 {
    let lo = u32::from(nes.peek(cfg.target_addr));
    if cfg.two_byte {
        lo | (u32::from(nes.peek(cfg.target_addr.wrapping_add(1))) << 8)
    } else {
        lo
    }
}

/// Run the search from `nes`'s current state, returning the best-scoring input
/// sequence found.
///
/// The `nes` is restored to its starting (anchor) state on return, so the live
/// timeline is untouched. Each `progress(done, attempts)` call (if given) reports
/// incremental progress so a UI can show a bar; pass `None` for a plain headless run.
pub fn search(
    nes: &mut Nes,
    cfg: &BotConfig,
    mut progress: Option<&mut dyn FnMut(usize, usize)>,
) -> BotResult {
    let anchor = nes.snapshot();
    let mut rng = SplitMix64::new(cfg.seed);
    let mut best = BotResult::default();
    // Baseline: the target value with no input (so a search that can't improve still
    // reports the honest starting score rather than 0).
    if nes.restore(&anchor).is_ok() {
        best.best_score = read_target(nes, cfg);
    }
    let pool_len = cfg.pool.len();
    for attempt in 0..cfg.attempts {
        if nes.restore(&anchor).is_err() {
            break;
        }
        let mut inputs = Vec::with_capacity(cfg.frames);
        for _ in 0..cfg.frames {
            let b = if pool_len == 0 {
                Buttons::empty()
            } else {
                let idx = usize::try_from(rng.next_u64() % pool_len as u64).unwrap_or(0);
                cfg.pool[idx]
            };
            inputs.push(b);
            nes.set_buttons(0, b);
            nes.run_frame();
        }
        let score = read_target(nes, cfg);
        best.attempts_run = attempt + 1;
        if score > best.best_score {
            best.best_score = score;
            best.best_inputs = inputs;
        }
        if let Some(cb) = progress.as_deref_mut() {
            cb(attempt + 1, cfg.attempts);
        }
    }
    // Leave the emulator exactly where we found it.
    let _ = nes.restore(&anchor);
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth_nrom() -> Vec<u8> {
        // Minimal valid NROM iNES image (16 KiB PRG + 8 KiB CHR).
        let mut rom = vec![0x4E, 0x45, 0x53, 0x1A, 1, 1, 0, 0];
        rom.extend(std::iter::repeat_n(0u8, 8));
        rom.extend(std::iter::repeat_n(0u8, 16 * 1024));
        rom.extend(std::iter::repeat_n(0u8, 8 * 1024));
        rom
    }

    fn booted() -> Nes {
        let mut n = Nes::from_rom(&synth_nrom()).unwrap();
        n.power_cycle();
        n
    }

    #[test]
    fn search_is_deterministic_for_a_seed() {
        let cfg = BotConfig {
            frames: 8,
            attempts: 16,
            ..BotConfig::default()
        };
        let r1 = search(&mut booted(), &cfg, None);
        let r2 = search(&mut booted(), &cfg, None);
        assert_eq!(r1, r2);
        assert_eq!(r1.attempts_run, 16);
    }

    #[test]
    fn restores_the_anchor_state() {
        let mut nes = booted();
        let before = nes.snapshot();
        let cfg = BotConfig {
            frames: 4,
            attempts: 8,
            ..BotConfig::default()
        };
        let _ = search(&mut nes, &cfg, None);
        // The live state is byte-identical to before the search.
        assert_eq!(nes.snapshot(), before);
    }

    #[test]
    fn best_inputs_length_matches_frames_when_improved() {
        let cfg = BotConfig {
            frames: 5,
            attempts: 32,
            ..BotConfig::default()
        };
        let r = search(&mut booted(), &cfg, None);
        assert!(r.best_inputs.is_empty() || r.best_inputs.len() == 5);
    }
}

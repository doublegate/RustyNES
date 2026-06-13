//! A small seeded PRNG.
//!
//! The whole netplay stack — the rollback session, the in-memory test
//! transport's latency/jitter/drop model, and the tests' input generators —
//! must be **deterministic**. The emulator core's determinism contract
//! (same ROM + seed + input ⇒ byte-identical state) is the foundation of
//! rollback: re-simulating a frame must reproduce it bit-for-bit. If any
//! part of the netcode pulled from `std::time` or the OS RNG, two peers
//! (or a peer and the reference run) could diverge for reasons unrelated to
//! input, defeating the harness.
//!
//! So this module provides a `SplitMix64` generator — tiny, fast, and with
//! no global state. It is the **only** source of randomness anywhere in the
//! crate. It is not cryptographically secure; it does not need to be.

/// A seeded `SplitMix64` pseudo-random number generator.
///
/// Deterministic: the same seed yields the same sequence on every platform.
/// Used for transport latency/jitter/drop simulation and for test input
/// generation. Never seed this from `std::time` or the OS — that would break
/// the determinism contract the whole crate relies on.
#[derive(Clone, Debug)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// New generator seeded with `seed`.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Next 64-bit value.
    pub const fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Next value in `0..bound` (`bound` must be non-zero). Uniform enough
    /// for jitter/drop simulation; not bias-free for cryptographic use.
    pub fn next_below(&mut self, bound: u32) -> u32 {
        debug_assert!(bound > 0, "next_below bound must be non-zero");
        // The remainder is `< bound <= u32::MAX`, so the conversion never
        // fails; `unwrap_or(0)` is just a panic-free fallback.
        u32::try_from(self.next_u64() % u64::from(bound)).unwrap_or(0)
    }

    /// Next value as a probability in `[0.0, 1.0)`.
    pub fn next_unit(&mut self) -> f64 {
        // Take the top 53 bits so the integer fits the f64 mantissa exactly
        // (the casts below are therefore lossless).
        #[allow(clippy::cast_precision_loss)]
        {
            (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    /// Next byte — handy for generating pseudo-random controller input.
    pub const fn next_u8(&mut self) -> u8 {
        (self.next_u64() >> 56) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_for_same_seed() {
        let mut a = SplitMix64::new(0xDEAD_BEEF);
        let mut b = SplitMix64::new(0xDEAD_BEEF);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn differs_for_different_seed() {
        let mut a = SplitMix64::new(1);
        let mut b = SplitMix64::new(2);
        let mut differ = false;
        for _ in 0..100 {
            if a.next_u64() != b.next_u64() {
                differ = true;
                break;
            }
        }
        assert!(differ);
    }

    #[test]
    fn next_below_in_range() {
        let mut r = SplitMix64::new(42);
        for _ in 0..1000 {
            assert!(r.next_below(7) < 7);
        }
    }
}

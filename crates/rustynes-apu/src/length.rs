//! Length-counter sub-unit (shared by pulse, triangle, noise).
//!
//! Per `docs/apu-2a03.md` §State and the NESdev wiki "APU Length Counter"
//! page. A 5-bit register selects from a fixed 32-entry lookup table; when
//! non-zero and clocked at half-frame, decrements toward zero. A `halt` bit
//! freezes the counter (also doubles as the envelope-loop bit on pulse and
//! noise channels).

/// 32-entry length lookup table (from the NESdev wiki).
pub const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

/// Length counter shared by pulse, triangle, noise channels.
#[derive(Debug, Clone, Copy, Default)]
pub struct LengthCounter {
    /// Current count (0..=254). 0 = silenced.
    pub count: u8,
    /// Halt flag (also serves as envelope-loop on pulse/noise; control on tri).
    pub halt: bool,
    /// Channel-enable flag from `$4015` write.
    pub enabled: bool,
}

impl LengthCounter {
    /// Load a new value from a `$4003`/`$4007`/`$400B`/`$400F` write.
    /// Lookup index = top 5 bits of the value.
    pub fn load(&mut self, raw: u8) {
        if self.enabled {
            self.count = LENGTH_TABLE[(raw >> 3) as usize];
        }
    }

    /// Channel-enable update from `$4015` write. Clearing the bit forces
    /// the count to 0 (silences the channel).
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.count = 0;
        }
    }

    /// Half-frame clock.
    pub fn clock(&mut self) {
        if !self.halt && self.count > 0 {
            self.count -= 1;
        }
    }

    /// `$4015` read — bit set if count > 0.
    #[must_use]
    pub const fn active(&self) -> bool {
        self.count > 0
    }
}

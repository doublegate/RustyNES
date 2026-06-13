//! Optional non-standard input-device overlays for the `$4016`/`$4017`
//! controller ports: the Arkanoid "Vaus" paddle and the NES Zapper light gun.
//!
//! These are **opt-in overlays**. The bus holds an `Option<InputDevice>` per
//! port; when a port has no overlay device (the default) the standard
//! controller / Four Score serial path runs completely unchanged, so the
//! default + Four Score reads stay byte-identical and the determinism
//! contract is preserved. A device is only consulted when explicitly attached
//! via [`crate::Nes::set_paddle`] / [`crate::Nes::set_zapper`].
//!
//! ## Vaus paddle (Arkanoid controller)
//!
//! Per the `NESdev` "Arkanoid controller" page (NES 7-pin version), the device
//! reports on the player-2 port (`$4017`):
//!
//! ```text
//! 7  bit  0
//! ---- ----
//! xxxD Bxxx
//!    | |
//!    | +---- Fire button (1: pressed)        -> bit 3
//!    +------ Serial control knob data        -> bit 4
//!            (8/9-bit, inverted, MSb first)
//! ```
//!
//! A write of `$4016` bit 0 = 1 -> 0 (the standard controller strobe) starts a
//! "conversion": the 8-bit potentiometer value is latched MSb-first into the
//! shift register. Each `$4017` read shifts out the next bit (on bit 4),
//! **inverted** on the wire. After the register empties, reads repeat the
//! serial-in bit (the 9th / `LSb`). The fire button (bit 3) is returned directly
//! and is unaffected by the strobe.
//!
//! The in-tree `vaus-test` ROM (Damian Yerrick) documents the NES wiring as
//! `$4017 D3: Button`, `$4017 D4: Position (8 bits, MSB first)` — matching the
//! wiki layout above.
//!
//! ## Zapper light gun
//!
//! Per the `NESdev` "Zapper" page (NES variant), the device reports on its port:
//!
//! ```text
//! 7  bit  0
//! ---- ----
//! xxxT Wxxx
//!    | |
//!    | +---- Light sensed (0: detected; 1: NOT detected)  -> bit 3
//!    +------ Trigger (1: pulled/half-pulled; 0: released)  -> bit 4
//! ```
//!
//! Note the inverted light polarity: bit 3 is **0** while light is detected and
//! **1** otherwise. The light sensor stays active for roughly 19-26 scanlines
//! after seeing a bright pixel (the photodiode capacitor drains exponentially);
//! we use a simpler frame-granular model: a luminance threshold sampled at the
//! aim point once per completed frame (sufficient because games re-sample every
//! frame). The Zapper has no shift register — its byte is read in parallel and
//! is independent of the strobe.

/// The Arkanoid "Vaus" paddle overlay state.
///
/// Models the NES 7-pin variant on `$4017`: an 8-bit potentiometer value
/// shifted out MSb-first (inverted on the wire) on bit 4, plus a fire button
/// on bit 3.
#[derive(Clone, Copy, Debug)]
pub struct VausState {
    /// The raw (pre-inversion) 8-bit potentiometer position. `$00` is the far
    /// left, `$FF` the far right (per the wiki, turning right increases the
    /// value).
    pub(crate) position: u8,
    /// Whether the fire button is currently held.
    pub(crate) fire: bool,
    /// 8-bit shift register, MSb-first readout. Reloaded from `position` on the
    /// strobe falling edge (conversion latch). The serial-in bit (repeated
    /// after the register empties) is the current `LSb`.
    pub(crate) shift: u8,
    /// Last strobe level written (bit 0 of `$4016`).
    pub(crate) strobe: bool,
}

impl Default for VausState {
    fn default() -> Self {
        Self::new()
    }
}

impl VausState {
    /// New paddle centered, button released.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            position: 0x80,
            fire: false,
            shift: 0x80,
            strobe: false,
        }
    }

    /// Update the live paddle position + fire state. Takes effect on the next
    /// conversion (strobe falling edge), matching the standard controller's
    /// latch-on-strobe semantics.
    pub const fn set(&mut self, position: u8, fire: bool) {
        self.position = position;
        self.fire = fire;
        if self.strobe {
            self.shift = position;
        }
    }

    /// Handle a `$4016` strobe write. On the rising edge the conversion latches
    /// the current position into the shift register (we model the conversion as
    /// instantaneous, which is the standard fixed-position emulation choice).
    pub const fn write_strobe(&mut self, value: u8) {
        let new_strobe = value & 1 != 0;
        if new_strobe {
            self.shift = self.position;
        }
        self.strobe = new_strobe;
    }

    /// Read the device byte for a `$4017` access, advancing the shift register.
    /// Returns the full 8-bit value already positioned on bits 3 (fire) and 4
    /// (knob data); the caller ORs in the open-bus upper bits.
    ///
    /// Bit 4 carries the **inverted** `MSb` of the shift register (knob data is
    /// inverted on the wire per the wiki). Bit 3 carries the fire button (1 =
    /// pressed). All other bits are 0.
    pub const fn read(&mut self) -> u8 {
        // Knob data bit: MSb of the shift register, inverted on the wire.
        let data_bit = (self.shift >> 7) & 1;
        let wire_data = data_bit ^ 1;
        // Shift left, feeding the LSb back into the serial-in position so that
        // post-empty reads repeat the 9th (serial-in) bit per the wiki.
        let serial_in = self.shift & 1;
        self.shift = (self.shift << 1) | serial_in;
        let fire = self.fire as u8;
        (wire_data << 4) | (fire << 3)
    }

    /// Side-effect-free sample of the next device byte (debugger peek).
    #[must_use]
    pub const fn peek(&self) -> u8 {
        let data_bit = (self.shift >> 7) & 1;
        let wire_data = data_bit ^ 1;
        let fire = self.fire as u8;
        (wire_data << 4) | (fire << 3)
    }

    /// Reconstruct from save-state parts.
    #[must_use]
    pub const fn from_parts(position: u8, fire: bool, shift: u8, strobe: bool) -> Self {
        Self {
            position,
            fire,
            shift,
            strobe,
        }
    }

    /// Raw potentiometer position (save-state).
    #[must_use]
    pub const fn position_raw(&self) -> u8 {
        self.position
    }
    /// Raw fire state (save-state).
    #[must_use]
    pub const fn fire_raw(&self) -> bool {
        self.fire
    }
    /// Raw shift register (save-state).
    #[must_use]
    pub const fn shift_raw(&self) -> u8 {
        self.shift
    }
    /// Raw strobe state (save-state).
    #[must_use]
    pub const fn strobe_raw(&self) -> bool {
        self.strobe
    }
}

/// The NES Zapper light-gun overlay state.
///
/// Models the NES variant: bit 3 = light sensed (0 detected / 1 not), bit 4 =
/// trigger (1 pulled). Light detection samples the PPU framebuffer luminance at
/// the aim point once per frame (a frame-granular model — see `light_seen`).
#[derive(Clone, Copy, Debug, Default)]
pub struct ZapperState {
    /// Aim point X (0..256), screen pixel. Out-of-range = aimed off-screen.
    pub(crate) x: u16,
    /// Aim point Y (0..240), screen scanline. Out-of-range = aimed off-screen.
    pub(crate) y: u16,
    /// Whether the trigger is currently pulled.
    pub(crate) trigger: bool,
    /// Whether the photodiode currently sees light. Set by [`Self::sample_light`]
    /// each frame from the framebuffer luminance at the aim point; while `true`,
    /// bit 3 reads 0 (light detected), else bit 3 reads 1 (no light). This is a
    /// frame-granular model: games re-sample every frame, so per-frame
    /// resolution is sufficient (the wiki's ~19-26-scanline photodiode hold
    /// matters only for sub-frame timing tricks, which the supported games do
    /// not require).
    pub(crate) light_seen: bool,
}

/// Luminance threshold (0..255, Rec.601-ish) above which a sampled framebuffer
/// pixel counts as "bright enough" to trigger the photodiode.
pub(crate) const ZAPPER_LUMA_THRESHOLD: u16 = 0x80;

impl ZapperState {
    /// New zapper aimed off-screen, trigger released, no light.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            x: u16::MAX,
            y: u16::MAX,
            trigger: false,
            light_seen: false,
        }
    }

    /// Update the live aim point + trigger state.
    pub const fn set(&mut self, x: u16, y: u16, trigger: bool) {
        self.x = x;
        self.y = y;
        self.trigger = trigger;
    }

    /// Sample the framebuffer luminance at the aim point, setting `light_seen`
    /// if the pixel is bright enough. `framebuffer` is the PPU's RGBA8 256x240
    /// buffer. Called once per frame by the bus after the frame completes.
    pub fn sample_light(&mut self, framebuffer: &[u8]) {
        const W: usize = 256;
        const H: usize = 240;
        if (self.x as usize) >= W || (self.y as usize) >= H {
            // Aimed off-screen: never sees light.
            self.light_seen = false;
            return;
        }
        let idx = ((self.y as usize) * W + (self.x as usize)) * 4;
        // Guard against a partial framebuffer.
        if idx + 2 >= framebuffer.len() {
            self.light_seen = false;
            return;
        }
        let r = u16::from(framebuffer[idx]);
        let g = u16::from(framebuffer[idx + 1]);
        let b = u16::from(framebuffer[idx + 2]);
        // Rec.601 luma approximation (integer): (77*R + 150*G + 29*B) >> 8.
        let luma = (77 * r + 150 * g + 29 * b) >> 8;
        self.light_seen = luma >= ZAPPER_LUMA_THRESHOLD;
    }

    /// The device byte for a `$4016`/`$4017` access. Bit 3 = light (0 detected /
    /// 1 not), bit 4 = trigger (1 pulled). Independent of the strobe (the
    /// Zapper has no shift register). The caller ORs in the open-bus upper bits.
    #[must_use]
    pub const fn read(&self) -> u8 {
        let light_not_detected = (!self.light_seen) as u8;
        let trigger = self.trigger as u8;
        (trigger << 4) | (light_not_detected << 3)
    }

    /// Reconstruct from save-state parts.
    #[must_use]
    pub const fn from_parts(x: u16, y: u16, trigger: bool, light_seen: bool) -> Self {
        Self {
            x,
            y,
            trigger,
            light_seen,
        }
    }

    /// Raw aim X (save-state).
    #[must_use]
    pub const fn x_raw(&self) -> u16 {
        self.x
    }
    /// Raw aim Y (save-state).
    #[must_use]
    pub const fn y_raw(&self) -> u16 {
        self.y
    }
    /// Raw trigger state (save-state).
    #[must_use]
    pub const fn trigger_raw(&self) -> bool {
        self.trigger
    }
    /// Raw light-seen state (save-state).
    #[must_use]
    pub const fn light_seen_raw(&self) -> bool {
        self.light_seen
    }
}

/// An optional non-standard device overlaid on a controller port. When set,
/// the bus's `$4016`/`$4017` read path returns this device's byte instead of
/// the standard controller / Four Score serial byte.
#[derive(Clone, Copy, Debug)]
pub enum InputDevice {
    /// NES Zapper light gun.
    Zapper(ZapperState),
    /// Arkanoid "Vaus" paddle.
    Vaus(VausState),
}

impl InputDevice {
    /// Forward a `$4016` strobe write to the device (only the Vaus latches on
    /// it; the Zapper ignores it).
    pub const fn write_strobe(&mut self, value: u8) {
        match self {
            Self::Vaus(v) => v.write_strobe(value),
            Self::Zapper(_) => {}
        }
    }

    /// Read the device byte (already bit-positioned), advancing any internal
    /// shift register.
    pub const fn read(&mut self) -> u8 {
        match self {
            Self::Vaus(v) => v.read(),
            Self::Zapper(z) => z.read(),
        }
    }

    /// Side-effect-free sample of the device byte (debugger peek).
    #[must_use]
    pub const fn peek(&self) -> u8 {
        match self {
            Self::Vaus(v) => v.peek(),
            Self::Zapper(z) => z.read(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vaus_fire_button_on_bit3_independent_of_strobe() {
        let mut v = VausState::new();
        v.set(0x80, true);
        // No strobe yet; fire is returned directly regardless.
        assert_eq!(v.read() & (1 << 3), 1 << 3, "fire = bit 3 set");
        v.set(0x80, false);
        assert_eq!(v.read() & (1 << 3), 0, "fire released = bit 3 clear");
    }

    #[test]
    fn vaus_knob_shifts_out_msb_first_inverted_on_bit4() {
        let mut v = VausState::new();
        // position 0b1010_0000: MSb-first raw bits = 1,0,1,0,0,0,0,0
        v.set(0b1010_0000, false);
        v.write_strobe(1);
        v.write_strobe(0);
        // Wire is inverted, so expected wire bits (bit 4) = 0,1,0,1,1,1,1,1.
        let expect_raw = [1u8, 0, 1, 0, 0, 0, 0, 0];
        for (i, raw) in expect_raw.iter().enumerate() {
            let byte = v.read();
            let wire_bit = (byte >> 4) & 1;
            assert_eq!(wire_bit, raw ^ 1, "read {i}: wire bit must be inverted raw");
        }
    }

    #[test]
    fn vaus_post_empty_repeats_serial_in_bit() {
        let mut v = VausState::new();
        // LSb (serial-in) = 1 -> after the 8 real bits, reads repeat inverted 1 = 0.
        v.set(0b0000_0001, false);
        v.write_strobe(1);
        v.write_strobe(0);
        for _ in 0..8 {
            let _ = v.read();
        }
        // Now the register is all serial-in (1); wire bit = inverted = 0.
        for _ in 0..4 {
            assert_eq!((v.read() >> 4) & 1, 0);
        }
    }

    #[test]
    fn zapper_light_detected_for_bright_pixel() {
        let mut z = ZapperState::new();
        z.set(10, 10, false);
        let mut fb = alloc::vec![0u8; 256 * 240 * 4];
        // Bright white pixel at (10, 10).
        let idx = (10 * 256 + 10) * 4;
        fb[idx] = 0xFF;
        fb[idx + 1] = 0xFF;
        fb[idx + 2] = 0xFF;
        z.sample_light(&fb);
        // Light detected -> bit 3 = 0.
        assert_eq!(
            z.read() & (1 << 3),
            0,
            "bright pixel -> light detected (bit3=0)"
        );
    }

    #[test]
    fn zapper_no_light_for_dark_pixel() {
        let mut z = ZapperState::new();
        z.set(10, 10, false);
        let fb = alloc::vec![0u8; 256 * 240 * 4]; // all black
        z.sample_light(&fb);
        assert_eq!(
            z.read() & (1 << 3),
            1 << 3,
            "dark pixel -> no light (bit3=1)"
        );
    }

    #[test]
    fn zapper_off_screen_never_sees_light() {
        let mut z = ZapperState::new();
        z.set(1000, 1000, false);
        let mut fb = alloc::vec![0u8; 256 * 240 * 4];
        fb.fill(0xFF);
        z.sample_light(&fb);
        assert_eq!(z.read() & (1 << 3), 1 << 3, "off-screen aim -> no light");
    }

    #[test]
    fn zapper_trigger_on_bit4() {
        let mut z = ZapperState::new();
        z.set(10, 10, true);
        assert_eq!(z.read() & (1 << 4), 1 << 4, "trigger pulled -> bit4 set");
        z.set(10, 10, false);
        assert_eq!(z.read() & (1 << 4), 0, "trigger released -> bit4 clear");
    }

    #[test]
    fn input_device_enum_dispatch() {
        let mut d = InputDevice::Vaus(VausState::new());
        d.write_strobe(1);
        d.write_strobe(0);
        let _ = d.read();
        let mut z = InputDevice::Zapper(ZapperState::new());
        // Strobe is a no-op for the Zapper.
        z.write_strobe(1);
        assert_eq!(z.read() & (1 << 3), 1 << 3, "zapper: no light by default");
    }
}

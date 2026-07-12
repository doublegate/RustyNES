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

/// Photodiode **aperture radius** in pixels (v2.2.0 "Capstone" light-timing
/// hardening). The real Zapper's lens focuses light from a small solid angle
/// onto the photodiode, so the sensor integrates a *region* of the CRT phosphor,
/// not a single dot. Sampling a `(2r+1) x (2r+1)` window around the aim point
/// (rather than one pixel) hardens detection against sub-pixel aim error and
/// single-pixel dropouts in the PPU output — matching how the hardware responds
/// to the bright target the game flashes. Radius 1 = a 3x3 aperture.
pub(crate) const ZAPPER_APERTURE_RADIUS: i32 = 1;

/// Minimum number of bright pixels within the aperture required to assert
/// "light detected". Requiring more than one rejects a lone stray-bright pixel
/// (PPU edge artefact) as a false positive while still firing on the target
/// flash, which lights the whole aperture. Calibrated for the 3x3 aperture.
pub(crate) const ZAPPER_APERTURE_MIN_BRIGHT: u32 = 2;

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

    /// Sample the framebuffer luminance over the photodiode **aperture** around
    /// the aim point, setting `light_seen` when enough of the aperture is bright.
    /// `framebuffer` is the PPU's RGBA8 256x240 buffer. Called once per frame by
    /// the bus after the frame completes.
    ///
    /// v2.2.0 "Capstone" light-timing hardening: rather than sampling a single
    /// pixel, the sensor integrates a `(2r+1) x (2r+1)` aperture
    /// ([`ZAPPER_APERTURE_RADIUS`]) and asserts light only when at least
    /// [`ZAPPER_APERTURE_MIN_BRIGHT`] pixels cross [`ZAPPER_LUMA_THRESHOLD`]. This
    /// models the lens/photodiode field-of-view against the PPU's per-dot output:
    /// the bright target the game flashes lights the whole aperture (robust
    /// detection), while a black "blanked" background frame — or a lone stray
    /// bright pixel — yields no light (no false positive). The computation is a
    /// pure, deterministic function of the framebuffer + aim point, so it needs
    /// no additional save-state and preserves the determinism contract.
    ///
    /// The temporal light-sense window (the ~19-26-scanline photodiode hold) is
    /// finer than the per-frame sample resolution used here; the supported
    /// light-gun titles re-poll every frame, so frame-granular sampling of the
    /// presented framebuffer is sufficient. A full per-dot temporal integration
    /// against the beam position is a documented future refinement — see
    /// `docs/frontend.md`.
    pub fn sample_light(&mut self, framebuffer: &[u8]) {
        const W: i32 = 256;
        const H: i32 = 240;
        let (ax, ay) = (i32::from(self.x), i32::from(self.y));
        if ax >= W || ay >= H {
            // Aimed off-screen: never sees light.
            self.light_seen = false;
            return;
        }
        let mut bright = 0u32;
        let r = ZAPPER_APERTURE_RADIUS;
        for dy in -r..=r {
            for dx in -r..=r {
                let (px, py) = (ax + dx, ay + dy);
                if !(0..W).contains(&px) || !(0..H).contains(&py) {
                    continue; // aperture clipped by the screen edge
                }
                // px/py are now bounded to the screen, so the linear index is
                // non-negative and fits a usize.
                let Ok(idx) = usize::try_from((py * W + px) * 4) else {
                    continue;
                };
                if idx + 2 >= framebuffer.len() {
                    continue; // guard against a partial framebuffer
                }
                let cr = u16::from(framebuffer[idx]);
                let cg = u16::from(framebuffer[idx + 1]);
                let cb = u16::from(framebuffer[idx + 2]);
                // Rec.601 luma approximation (integer): (77*R + 150*G + 29*B) >> 8.
                let luma = (77 * cr + 150 * cg + 29 * cb) >> 8;
                if luma >= ZAPPER_LUMA_THRESHOLD {
                    bright += 1;
                }
            }
        }
        self.light_seen = bright >= ZAPPER_APERTURE_MIN_BRIGHT;
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

/// The NES Power Pad (a.k.a. Family Fun Fitness / Family Trainer mat) overlay.
///
/// A 12-button mat read on the player-2 port (`$4017`) through two 8-bit
/// parallel-in/serial-out shift registers (a pair of 4021s), strobed by the
/// standard `$4016` controller strobe. The 12 buttons are indexed 0..=11
/// (matching the mat's "1".."12" labels); the frontend decides which physical
/// keys map to which mat button (and any Side-A/Side-B row inversion).
///
/// Per the `NESdev` "Power Pad" page (and Mesen's implementation), the button
/// bits load into two registers and shift out LSb-first on bits 3 and 4 of each
/// `$4017` read:
///
/// - register L (bit 3 of the read): buttons 2, 1, 5, 9, 6, 10, 11, 7;
/// - register H (bit 4 of the read): buttons 4, 3, 12, 8, then four `1` bits.
///
/// (Button numbers are 1-based here, matching the mat labels; the code uses
/// 0-based indices.) Each read shifts both registers right and feeds `1`s in
/// from the top, so post-shift reads settle to "no button".
#[derive(Clone, Copy, Debug, Default)]
pub struct PowerPadState {
    /// Live pressed-button mask: bit `i` (0..=11) set = mat button `i+1` held.
    pub(crate) buttons: u16,
    /// Low shift register (read out on bit 3).
    pub(crate) shift_l: u8,
    /// High shift register (read out on bit 4).
    pub(crate) shift_h: u8,
    /// Last strobe level written (bit 0 of `$4016`).
    pub(crate) strobe: bool,
}

impl PowerPadState {
    /// New mat with no buttons pressed.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buttons: 0,
            shift_l: 0,
            shift_h: 0,
            strobe: false,
        }
    }

    /// Reload both shift registers from the live button mask (the parallel
    /// latch). The bit order matches the `NESdev` / Mesen serial layout.
    const fn reload(&mut self) {
        // bit(i) = (buttons >> i) & 1, as u8. Inlined (no closures in const fn).
        let p = self.buttons;
        // L: buttons 2,1,5,9,6,10,11,7 (0-based 1,0,4,8,5,9,10,6).
        self.shift_l = (((p >> 1) & 1) as u8)
            | (((p & 1) as u8) << 1)
            | ((((p >> 4) & 1) as u8) << 2)
            | ((((p >> 8) & 1) as u8) << 3)
            | ((((p >> 5) & 1) as u8) << 4)
            | ((((p >> 9) & 1) as u8) << 5)
            | ((((p >> 10) & 1) as u8) << 6)
            | ((((p >> 6) & 1) as u8) << 7);
        // H: buttons 4,3,12,8 (0-based 3,2,11,7), then four 1 bits (read as H=1).
        self.shift_h = (((p >> 3) & 1) as u8)
            | ((((p >> 2) & 1) as u8) << 1)
            | ((((p >> 11) & 1) as u8) << 2)
            | ((((p >> 7) & 1) as u8) << 3)
            | 0xF0;
    }

    /// Update the live pressed-button mask (bit `i` = mat button `i+1`). While
    /// the strobe is held high the registers track the live mask (parallel
    /// load), matching the standard controller's latch-while-strobed semantics.
    pub const fn set(&mut self, buttons: u16) {
        self.buttons = buttons & 0x0FFF;
        if self.strobe {
            self.reload();
        }
    }

    /// Handle a `$4016` strobe write. While bit 0 is high the registers are
    /// (re)loaded from the live buttons; the falling edge leaves the latched
    /// snapshot to shift out.
    pub const fn write_strobe(&mut self, value: u8) {
        let new_strobe = value & 1 != 0;
        if new_strobe {
            self.reload();
        }
        self.strobe = new_strobe;
    }

    /// Read the device byte for a `$4017` access, shifting both registers.
    /// Bit 4 = the current serial-out (`LSb`) of register H, bit 3 = register L;
    /// each read then shifts both right (feeding `1`s in from the top). The
    /// caller ORs in the open-bus upper bits. While the strobe is high the
    /// registers are continuously reloaded (reads return the first button).
    pub const fn read(&mut self) -> u8 {
        if self.strobe {
            self.reload();
        }
        let out = ((self.shift_h & 1) << 4) | ((self.shift_l & 1) << 3);
        self.shift_l = (self.shift_l >> 1) | 0x80;
        self.shift_h = (self.shift_h >> 1) | 0x80;
        out
    }

    /// Side-effect-free sample of the next device byte (debugger peek).
    #[must_use]
    pub const fn peek(&self) -> u8 {
        ((self.shift_h & 1) << 4) | ((self.shift_l & 1) << 3)
    }

    /// Reconstruct from save-state parts. `buttons` is masked to the 12 mat
    /// bits, matching [`Self::set`], so a malformed save-state cannot inject
    /// out-of-range bits.
    #[must_use]
    pub const fn from_parts(buttons: u16, shift_l: u8, shift_h: u8, strobe: bool) -> Self {
        Self {
            buttons: buttons & 0x0FFF,
            shift_l,
            shift_h,
            strobe,
        }
    }

    /// Raw live button mask (save-state).
    #[must_use]
    pub const fn buttons_raw(&self) -> u16 {
        self.buttons
    }
    /// Raw low shift register (save-state).
    #[must_use]
    pub const fn shift_l_raw(&self) -> u8 {
        self.shift_l
    }
    /// Raw high shift register (save-state).
    #[must_use]
    pub const fn shift_h_raw(&self) -> u8 {
        self.shift_h
    }
    /// Raw strobe state (save-state).
    #[must_use]
    pub const fn strobe_raw(&self) -> bool {
        self.strobe
    }
}

/// The (Hyperkin / Nintendo) mouse overlay state — the SNES-style serial mouse
/// as wired to an NES `$4016`/`$4017` port (D0 serial-out).
///
/// Per the `NESdev` "Mouse" page (the SNES mouse, the canonical serial mouse
/// reused on the NES), a strobe latches a fixed-format 32-bit report that is
/// then shifted out **MSb-first on D0** (one bit per port read):
///
/// ```text
/// bits 31..28 : signature 0b0001 (device id nibble)
/// bits 27..26 : 00
/// bits 25..24 : sensitivity (00 low / 01 medium / 10 high; cycled by pressing
///               both buttons on real hardware — we expose it as a field)
/// bit  23     : left button  (1 = pressed)
/// bit  22     : right button (1 = pressed)
/// bits 21..16 : 0
/// bits 15..8  : Y movement — bit 15 = direction sign (1 = up/-), bits 14..8 =
///               magnitude (0..127); 0 when not moving
/// bits  7..0  : X movement — bit  7 = direction sign (1 = left/-), bits  6..0 =
///               magnitude (0..127); 0 when not moving
/// ```
///
/// After the 32 real bits are shifted out, further reads return `1` (the open
/// serial line idles high), matching the standard controller's post-sequence
/// behavior. Like the standard controller, while the strobe is held high the
/// report is continuously re-latched, so reads return the first (signature) bit.
#[derive(Clone, Copy, Debug, Default)]
pub struct SnesMouseState {
    /// Live delta-X this frame (signed; clamped into +/-127 on latch).
    pub(crate) dx: i16,
    /// Live delta-Y this frame (signed; clamped into +/-127 on latch).
    pub(crate) dy: i16,
    /// Left button held.
    pub(crate) left: bool,
    /// Right button held.
    pub(crate) right: bool,
    /// Sensitivity (0 low / 1 medium / 2 high). Reported in the latched word.
    pub(crate) sensitivity: u8,
    /// 32-bit shift register, MSb-first readout. Reloaded from the live state on
    /// the strobe (the parallel latch).
    pub(crate) shift: u32,
    /// Count of real bits shifted out (0..=32); beyond 32, reads idle high (`1`).
    pub(crate) read_count: u8,
    /// Last strobe level written (bit 0 of `$4016`).
    pub(crate) strobe: bool,
}

impl SnesMouseState {
    /// New mouse at rest (no movement, buttons up, low sensitivity).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            dx: 0,
            dy: 0,
            left: false,
            right: false,
            sensitivity: 0,
            shift: 0,
            read_count: 0,
            strobe: false,
        }
    }

    /// Encode one axis into the 8-bit serial field: bit 7 = direction sign
    /// (1 = negative), bits 6..0 = magnitude clamped to 0..=127.
    const fn enc_axis(v: i16) -> u32 {
        // `v.unsigned_abs()` avoids the `-i16::MIN` overflow panic that `-v`
        // would hit for `v == i16::MIN` (32768 is unrepresentable as `i16`).
        let mag = v.unsigned_abs() as u32;
        let mag = if mag > 127 { 127 } else { mag };
        let sign = if v < 0 { 1u32 } else { 0 };
        (sign << 7) | mag
    }

    /// Encode the current live state into the 32-bit report word (MSb-first
    /// serial order; bit 31 is shifted out first).
    const fn encode(&self) -> u32 {
        let dx = Self::enc_axis(self.dx);
        let dy = Self::enc_axis(self.dy);
        let sig = 0b0001u32 << 28;
        let sens = ((self.sensitivity & 0b11) as u32) << 24;
        let left = (self.left as u32) << 23;
        let right = (self.right as u32) << 22;
        sig | sens | left | right | (dy << 8) | dx
    }

    /// Update the live movement + button + sensitivity state. Takes effect on
    /// the next latch (strobe), matching the standard controller semantics.
    pub const fn set(&mut self, dx: i16, dy: i16, left: bool, right: bool, sensitivity: u8) {
        self.dx = dx;
        self.dy = dy;
        self.left = left;
        self.right = right;
        self.sensitivity = sensitivity & 0b11;
        if self.strobe {
            self.shift = self.encode();
            self.read_count = 0;
        }
    }

    /// Handle a `$4016` strobe write. On a high level the 32-bit report is
    /// (re)latched from the live state; the read counter resets.
    pub const fn write_strobe(&mut self, value: u8) {
        let new_strobe = value & 1 != 0;
        if new_strobe {
            self.shift = self.encode();
            self.read_count = 0;
        }
        self.strobe = new_strobe;
    }

    /// Read the device byte for a port access, shifting out one MSb-first bit on
    /// D0. After 32 bits the line idles high (`1` on D0). While the strobe is
    /// held high the report is continuously re-latched (reads return bit 31).
    /// The caller ORs in the open-bus upper bits.
    pub const fn read(&mut self) -> u8 {
        if self.strobe {
            self.shift = self.encode();
            self.read_count = 0;
        }
        if self.read_count >= 32 {
            return 1; // serial line idles high after the report
        }
        let bit = (self.shift >> 31) & 1;
        self.shift <<= 1;
        self.read_count += 1;
        bit as u8
    }

    /// Side-effect-free sample of the next D0 bit (debugger peek).
    #[must_use]
    pub const fn peek(&self) -> u8 {
        if self.read_count >= 32 {
            return 1;
        }
        ((self.shift >> 31) & 1) as u8
    }

    /// Reconstruct from save-state parts.
    #[must_use]
    #[allow(clippy::too_many_arguments)] // one arg per persisted field
    pub const fn from_parts(
        dx: i16,
        dy: i16,
        left: bool,
        right: bool,
        sensitivity: u8,
        shift: u32,
        read_count: u8,
        strobe: bool,
    ) -> Self {
        Self {
            dx,
            dy,
            left,
            right,
            sensitivity: sensitivity & 0b11,
            shift,
            read_count,
            strobe,
        }
    }

    /// Raw delta-X (save-state).
    #[must_use]
    pub const fn dx_raw(&self) -> i16 {
        self.dx
    }
    /// Raw delta-Y (save-state).
    #[must_use]
    pub const fn dy_raw(&self) -> i16 {
        self.dy
    }
    /// Raw left button (save-state).
    #[must_use]
    pub const fn left_raw(&self) -> bool {
        self.left
    }
    /// Raw right button (save-state).
    #[must_use]
    pub const fn right_raw(&self) -> bool {
        self.right
    }
    /// Raw sensitivity (save-state).
    #[must_use]
    pub const fn sensitivity_raw(&self) -> u8 {
        self.sensitivity
    }
    /// Raw shift register (save-state).
    #[must_use]
    pub const fn shift_raw(&self) -> u32 {
        self.shift
    }
    /// Raw read counter (save-state).
    #[must_use]
    pub const fn read_count_raw(&self) -> u8 {
        self.read_count
    }
    /// Raw strobe state (save-state).
    #[must_use]
    pub const fn strobe_raw(&self) -> bool {
        self.strobe
    }
}

/// Number of physical keys on the Famicom Family BASIC keyboard matrix
/// (`9 rows x 8 columns / 2` halves; 72 keys, with a handful of unused matrix
/// positions reported as `1` / not-pressed).
pub const FAMILY_KEYBOARD_KEYS: usize = 72;

/// Number of selectable rows in the Family BASIC keyboard matrix.
const FAMILY_KEYBOARD_ROWS: usize = 9;

/// The Famicom **Family BASIC keyboard** overlay state.
///
/// Per the `NESdev` "Family BASIC Keyboard" page, the keyboard is a `9 x 8`
/// switch matrix (with the data-recorder lines unused here) read through the
/// expansion port but software-visible on `$4017`. The protocol:
///
/// - **`$4016` write** — bit 0 (the "column" select; 0 selects the low 4 keys
///   of the current row, 1 selects the high 4) and bit 1 (a clock; a 0->1
///   transition advances to the next row). Bit 2 enables the keyboard matrix;
///   when bit 2 is 0 the matrix is disabled and `$4017` reads `1`s. Writing
///   bit 1 = 0 while bit 2 = 1 **resets** the row counter to 0.
/// - **`$4017` read** — bits 4..1 carry the four key switches of the currently
///   selected (row, column-half), **active-low** (0 = pressed). There are 9
///   rows x 2 halves = 18 selectable groups of 4 keys = 72 key positions.
///
/// We model the live pressed state as a 72-bit key bitmap (`[u8; 9]`, one byte
/// per row: low nibble = column-half 0, high nibble = column-half 1) and the
/// row counter + column select per the write protocol. Determinism holds: it is
/// a pure function of the writes + the live key bitmap.
#[derive(Clone, Copy, Debug)]
pub struct FamilyKeyboardState {
    /// Per-row key bitmap. `keys[row]` bits 0..=3 = column-half 0 keys, bits
    /// 4..=7 = column-half 1 keys. A set bit = that key is held.
    pub(crate) keys: [u8; FAMILY_KEYBOARD_ROWS],
    /// Current matrix row (0..=8); wraps/saturates at the last row.
    pub(crate) row: u8,
    /// Column-half select (bit 0 of the last `$4016` write): 0 = low nibble,
    /// 1 = high nibble.
    pub(crate) column: bool,
    /// Whether the matrix is enabled (bit 2 of the last `$4016` write). When
    /// disabled, `$4017` reads return all-`1` (no keys).
    pub(crate) enabled: bool,
    /// Last clock level (bit 1 of `$4016`); a 0->1 edge advances the row.
    pub(crate) clock: bool,
}

impl Default for FamilyKeyboardState {
    fn default() -> Self {
        Self::new()
    }
}

impl FamilyKeyboardState {
    /// New keyboard with no keys held, matrix reset + disabled.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            keys: [0; FAMILY_KEYBOARD_ROWS],
            row: 0,
            column: false,
            enabled: false,
            clock: false,
        }
    }

    /// Set the full pressed-key bitmap (one byte per matrix row; low nibble =
    /// column-half 0, high nibble = column-half 1). The frontend builds this
    /// from host keys via its key map.
    pub const fn set_keys(&mut self, keys: [u8; FAMILY_KEYBOARD_ROWS]) {
        self.keys = keys;
    }

    /// Set one key by linear index (0..72) — `index = row * 8 + bit`, matching
    /// the matrix layout (`bit` 0..=3 = column-half 0, 4..=7 = column-half 1).
    /// Out-of-range indices are ignored.
    pub const fn set_key(&mut self, index: usize, pressed: bool) {
        if index >= FAMILY_KEYBOARD_KEYS {
            return;
        }
        let row = index / 8;
        #[allow(clippy::cast_possible_truncation)] // index % 8 is always 0..=7
        let bit = (index % 8) as u8;
        if pressed {
            self.keys[row] |= 1 << bit;
        } else {
            self.keys[row] &= !(1 << bit);
        }
    }

    /// Handle a `$4016` write: latch column select (bit 0), advance the row on a
    /// clock (bit 1) rising edge, set the matrix-enable (bit 2). A clock low
    /// while enabled resets the row counter to 0.
    pub const fn write_strobe(&mut self, value: u8) {
        let column = value & 0x01 != 0;
        let clock = value & 0x02 != 0;
        let enabled = value & 0x04 != 0;
        if enabled {
            if clock && !self.clock {
                // Rising clock edge: advance to the next row (saturate at last).
                if (self.row as usize) < FAMILY_KEYBOARD_ROWS - 1 {
                    self.row += 1;
                }
            } else if !clock {
                // Clock low (while enabled): reset to the first row.
                self.row = 0;
            }
        }
        self.column = column;
        self.enabled = enabled;
        self.clock = clock;
    }

    /// Read the device byte for a `$4017` access. The four selected key switches
    /// are returned on bits 4..1, **active-low** (0 = pressed). When the matrix
    /// is disabled, all four bits read `1` (no keys). The caller ORs in the
    /// open-bus upper bits.
    #[must_use]
    pub const fn read(&self) -> u8 {
        if !self.enabled {
            // Disabled matrix: key switches all read high (not pressed).
            return 0b0001_1110;
        }
        let row = self.row as usize;
        let byte = self.keys[row];
        let nibble = if self.column {
            (byte >> 4) & 0x0F
        } else {
            byte & 0x0F
        };
        // Active-low: pressed key (1 in our bitmap) reads 0 on the wire.
        let wire = (!nibble) & 0x0F;
        wire << 1
    }

    /// Side-effect-free sample of the device byte (debugger peek) — identical to
    /// [`Self::read`] (the keyboard read has no side effects).
    #[must_use]
    pub const fn peek(&self) -> u8 {
        self.read()
    }

    /// Reconstruct from save-state parts.
    #[must_use]
    pub const fn from_parts(
        keys: [u8; FAMILY_KEYBOARD_ROWS],
        row: u8,
        column: bool,
        enabled: bool,
        clock: bool,
    ) -> Self {
        // Clamp the restored row to the matrix bound: a corrupt/malicious
        // save-state must not be able to drive `read()`'s `self.keys[row]`
        // out of bounds. The live `write_strobe` path already saturates the
        // row at `FAMILY_KEYBOARD_ROWS - 1`; mirror that on restore.
        let row = if (row as usize) >= FAMILY_KEYBOARD_ROWS {
            #[allow(clippy::cast_possible_truncation)] // ROWS is small (9)
            {
                (FAMILY_KEYBOARD_ROWS - 1) as u8
            }
        } else {
            row
        };
        Self {
            keys,
            row,
            column,
            enabled,
            clock,
        }
    }

    /// Raw per-row key bitmap (save-state).
    #[must_use]
    pub const fn keys_raw(&self) -> [u8; FAMILY_KEYBOARD_ROWS] {
        self.keys
    }
    /// Raw row counter (save-state).
    #[must_use]
    pub const fn row_raw(&self) -> u8 {
        self.row
    }
    /// Raw column select (save-state).
    #[must_use]
    pub const fn column_raw(&self) -> bool {
        self.column
    }
    /// Raw matrix-enable (save-state).
    #[must_use]
    pub const fn enabled_raw(&self) -> bool {
        self.enabled
    }
    /// Raw clock level (save-state).
    #[must_use]
    pub const fn clock_raw(&self) -> bool {
        self.clock
    }
}

/// The **Konami Hyper Shot** overlay state (v1.3.0 Workstream F1).
///
/// A simple 4-button expansion controller (two players, each with a Run and a
/// Jump button) used by _Hyper Olympic_ / _Hyper Sports_. Per the `NESdev`
/// "Konami Hyper Shot" page it is read in **parallel** on `$4017` (no shift
/// register), with `$4016` writes selecting which player's buttons are
/// enabled:
///
/// ```text
/// $4016 write:               $4017 read:
/// 7  bit  0                  7  bit  0
/// ---- ----                  ---- ----
/// xxxx xEFx                  xxxD CBAx
///       ||                      | |||
///       |+- 0 = enable P1          | ||+-- P1 Run
///       +-- 0 = enable P2          | |+--- P1 Jump
///                                  | +---- P2 Run
///                                  +------ P2 Jump
/// ```
///
/// The Jump/Run bits for a player read `0` while that player's enable bit
/// ($4016 bit 1 for P1, bit 2 for P2) is **set** (i.e. disabled). Determinism
/// holds: the read is a pure function of the live button mask + the last write.
#[derive(Clone, Copy, Debug, Default)]
pub struct KonamiHyperShotState {
    /// Live button mask: bit 0 = P1 Run, bit 1 = P1 Jump, bit 2 = P2 Run,
    /// bit 3 = P2 Jump.
    pub(crate) buttons: u8,
    /// `true` if P1's buttons are enabled (`$4016` bit 1 == 0).
    pub(crate) p1_enabled: bool,
    /// `true` if P2's buttons are enabled (`$4016` bit 2 == 0).
    pub(crate) p2_enabled: bool,
}

impl KonamiHyperShotState {
    /// New controller with no buttons held and both players enabled (the
    /// power-on `$4016` write has not happened yet; enable is active-low, so the
    /// quiescent state matches a write of 0).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buttons: 0,
            p1_enabled: true,
            p2_enabled: true,
        }
    }

    /// Set the live 4-button mask (bit 0 = P1 Run, 1 = P1 Jump, 2 = P2 Run,
    /// 3 = P2 Jump). Bits above 3 are ignored.
    pub const fn set(&mut self, buttons: u8) {
        self.buttons = buttons & 0x0F;
    }

    /// Handle a `$4016` write: bit 1 = 0 enables P1, bit 2 = 0 enables P2
    /// (active-low).
    pub const fn write_strobe(&mut self, value: u8) {
        self.p1_enabled = value & 0x02 == 0;
        self.p2_enabled = value & 0x04 == 0;
    }

    /// Read the device byte for a `$4017` access. Bit 1 = P1 Run, bit 2 = P1
    /// Jump, bit 3 = P2 Run, bit 4 = P2 Jump; a player's bits read `0` while
    /// disabled. The caller ORs in the open-bus upper bits.
    #[must_use]
    pub const fn read(&self) -> u8 {
        let p1_run = (self.buttons & 0x01 != 0) && self.p1_enabled;
        let p1_jump = (self.buttons & 0x02 != 0) && self.p1_enabled;
        let p2_run = (self.buttons & 0x04 != 0) && self.p2_enabled;
        let p2_jump = (self.buttons & 0x08 != 0) && self.p2_enabled;
        ((p1_run as u8) << 1)
            | ((p1_jump as u8) << 2)
            | ((p2_run as u8) << 3)
            | ((p2_jump as u8) << 4)
    }

    /// Side-effect-free sample (debugger peek) — identical to [`Self::read`].
    #[must_use]
    pub const fn peek(&self) -> u8 {
        self.read()
    }

    /// Reconstruct from save-state parts.
    #[must_use]
    pub const fn from_parts(buttons: u8, p1_enabled: bool, p2_enabled: bool) -> Self {
        Self {
            buttons: buttons & 0x0F,
            p1_enabled,
            p2_enabled,
        }
    }

    /// Raw button mask (save-state).
    #[must_use]
    pub const fn buttons_raw(&self) -> u8 {
        self.buttons
    }
    /// Raw P1-enable (save-state).
    #[must_use]
    pub const fn p1_enabled_raw(&self) -> bool {
        self.p1_enabled
    }
    /// Raw P2-enable (save-state).
    #[must_use]
    pub const fn p2_enabled_raw(&self) -> bool {
        self.p2_enabled
    }
}

/// The **Bandai Hyper Shot** (Exciting Boxing punching bag) overlay state
/// (v1.3.0 Workstream F1).
///
/// The punching bag has 8 sensors read on `$4017`, multiplexed by `$4016`
/// bit 1 (the "A" select) into two groups of four returned on bits 4..1. Per
/// the `NESdev` "Exciting Boxing Punching Bag" page:
///
/// ```text
/// $4016 write:               $4017 read:
/// 7  bit  0                  7  bit  0
/// ---- ----                  ---- ----
/// xxxx xxAx                  xxxE DCBx
///        |                      | |||
///        +- select group        | ||+-- Left Hook (A=0) / Left Jab  (A=1)
///                               | |+--- Move Right (A=0) / Body      (A=1)
///                               | +---- Move Left (A=0) / Right Jab  (A=1)
///                               +------ Right Hook (A=0) / Straight   (A=1)
/// ```
///
/// We model the 8 sensors as a live bitmask and the `A` select from the last
/// `$4016` write; the read is a pure function of both (deterministic).
#[derive(Clone, Copy, Debug, Default)]
pub struct BandaiHyperShotState {
    /// Live sensor mask. Group A=0 (bits 0..=3): Left Hook, Move Right, Move
    /// Left, Right Hook. Group A=1 (bits 4..=7): Left Jab, Body, Right Jab,
    /// Straight. A set bit = that sensor is active.
    pub(crate) sensors: u8,
    /// The `A` select latched from the last `$4016` write (bit 1). `false`
    /// selects the A=0 group (bits 0..=3), `true` the A=1 group (bits 4..=7).
    pub(crate) select: bool,
}

impl BandaiHyperShotState {
    /// New punching bag with no sensor active, group A=0 selected.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sensors: 0,
            select: false,
        }
    }

    /// Set the live 8-sensor mask. Bits 0..=3 are the A=0 group (Left Hook,
    /// Move Right, Move Left, Right Hook); bits 4..=7 are the A=1 group (Left
    /// Jab, Body, Right Jab, Straight).
    pub const fn set(&mut self, sensors: u8) {
        self.sensors = sensors;
    }

    /// Handle a `$4016` write: bit 1 (`A`) selects which sensor group is
    /// returned on the next reads.
    pub const fn write_strobe(&mut self, value: u8) {
        self.select = value & 0x02 != 0;
    }

    /// Read the device byte for a `$4017` access. The selected group's four
    /// sensors appear on bits 4..1. The caller ORs in the open-bus upper bits.
    #[must_use]
    pub const fn read(&self) -> u8 {
        let nibble = if self.select {
            (self.sensors >> 4) & 0x0F
        } else {
            self.sensors & 0x0F
        };
        nibble << 1
    }

    /// Side-effect-free sample (debugger peek) — identical to [`Self::read`].
    #[must_use]
    pub const fn peek(&self) -> u8 {
        self.read()
    }

    /// Reconstruct from save-state parts.
    #[must_use]
    pub const fn from_parts(sensors: u8, select: bool) -> Self {
        Self { sensors, select }
    }

    /// Raw sensor mask (save-state).
    #[must_use]
    pub const fn sensors_raw(&self) -> u8 {
        self.sensors
    }
    /// Raw `A`-select (save-state).
    #[must_use]
    pub const fn select_raw(&self) -> bool {
        self.select
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
    /// NES Power Pad / Family Fun Fitness mat (12 buttons).
    PowerPad(PowerPadState),
    /// SNES-style serial mouse (Hyperkin / Nintendo), D0 serial-out.
    SnesMouse(SnesMouseState),
    /// Famicom Family BASIC keyboard (72-key matrix on `$4017`).
    FamilyKeyboard(FamilyKeyboardState),
    /// Bandai **Family Trainer** mat (v1.3.0 Workstream F1). Layout-equivalent
    /// to the [`PowerPad`](Self::PowerPad): the Famicom mat reuses the exact
    /// 12-button parallel-in/serial-out scan (it differs only in the expansion-
    /// port wiring vs the NES controller-port Power Pad), so the same
    /// [`PowerPadState`] drives it.
    FamilyTrainer(PowerPadState),
    /// **Subor keyboard** (v1.3.0 Workstream F1). A Family BASIC keyboard
    /// work-alike (the Subor clone matrix), reusing the same
    /// [`FamilyKeyboardState`] `9 x 8` matrix scan.
    SuborKeyboard(FamilyKeyboardState),
    /// **Konami Hyper Shot** (v1.3.0 Workstream F1): a 4-button (2-player
    /// Run/Jump) parallel-read expansion controller.
    KonamiHyperShot(KonamiHyperShotState),
    /// **Bandai Hyper Shot** / Exciting Boxing punching bag (v1.3.0 Workstream
    /// F1): an 8-sensor expansion controller multiplexed into two groups.
    BandaiHyperShot(BandaiHyperShotState),
}

impl InputDevice {
    /// Forward a `$4016` strobe write to the device (only the Vaus latches on
    /// it; the Zapper ignores it).
    pub const fn write_strobe(&mut self, value: u8) {
        match self {
            Self::Vaus(v) => v.write_strobe(value),
            Self::PowerPad(p) | Self::FamilyTrainer(p) => p.write_strobe(value),
            Self::SnesMouse(m) => m.write_strobe(value),
            Self::FamilyKeyboard(k) | Self::SuborKeyboard(k) => k.write_strobe(value),
            Self::KonamiHyperShot(h) => h.write_strobe(value),
            Self::BandaiHyperShot(b) => b.write_strobe(value),
            Self::Zapper(_) => {}
        }
    }

    /// Read the device byte (already bit-positioned), advancing any internal
    /// shift register.
    pub const fn read(&mut self) -> u8 {
        match self {
            Self::Vaus(v) => v.read(),
            Self::Zapper(z) => z.read(),
            Self::PowerPad(p) | Self::FamilyTrainer(p) => p.read(),
            Self::SnesMouse(m) => m.read(),
            Self::FamilyKeyboard(k) | Self::SuborKeyboard(k) => k.read(),
            Self::KonamiHyperShot(h) => h.read(),
            Self::BandaiHyperShot(b) => b.read(),
        }
    }

    /// Side-effect-free sample of the device byte (debugger peek).
    #[must_use]
    pub const fn peek(&self) -> u8 {
        match self {
            Self::Vaus(v) => v.peek(),
            Self::Zapper(z) => z.read(),
            Self::PowerPad(p) | Self::FamilyTrainer(p) => p.peek(),
            Self::SnesMouse(m) => m.peek(),
            Self::FamilyKeyboard(k) | Self::SuborKeyboard(k) => k.peek(),
            Self::KonamiHyperShot(h) => h.peek(),
            Self::BandaiHyperShot(b) => b.peek(),
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
    fn zapper_light_detected_for_bright_region() {
        let mut z = ZapperState::new();
        z.set(10, 10, false);
        let mut fb = alloc::vec![0u8; 256 * 240 * 4];
        // Bright white 3x3 target block centred on the aim point (10, 10) — the
        // target flash lights the whole photodiode aperture.
        for py in 9..=11usize {
            for px in 9..=11usize {
                let idx = (py * 256 + px) * 4;
                fb[idx] = 0xFF;
                fb[idx + 1] = 0xFF;
                fb[idx + 2] = 0xFF;
            }
        }
        z.sample_light(&fb);
        // Light detected -> bit 3 = 0.
        assert_eq!(
            z.read() & (1 << 3),
            0,
            "bright target region -> light detected (bit3=0)"
        );
    }

    #[test]
    fn zapper_aperture_rejects_lone_bright_pixel() {
        // A single stray-bright pixel (below ZAPPER_APERTURE_MIN_BRIGHT) is not
        // enough to fire the photodiode — the aperture rejects PPU edge noise.
        let mut z = ZapperState::new();
        z.set(50, 50, false);
        let mut fb = alloc::vec![0u8; 256 * 240 * 4];
        let idx = (50 * 256 + 50) * 4;
        fb[idx] = 0xFF;
        fb[idx + 1] = 0xFF;
        fb[idx + 2] = 0xFF;
        z.sample_light(&fb);
        assert_eq!(
            z.read() & (1 << 3),
            1 << 3,
            "lone bright pixel -> no light (bit3=1)"
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

    /// Read 8 device bytes after a strobe, returning the bit-3 (L) and bit-4 (H)
    /// streams as bool arrays.
    fn powerpad_read8(p: &mut PowerPadState) -> ([bool; 8], [bool; 8]) {
        p.write_strobe(1);
        p.write_strobe(0);
        let mut l = [false; 8];
        let mut h = [false; 8];
        for i in 0..8 {
            let b = p.read();
            l[i] = b & (1 << 3) != 0;
            h[i] = b & (1 << 4) != 0;
        }
        (l, h)
    }

    #[test]
    fn powerpad_no_buttons_reads_clear_then_h_ones() {
        // No buttons: L is all 0; H reads 0 for the first 4 (buttons 4,3,12,8),
        // then 1 for the trailing "read as H=1" bits.
        let mut p = PowerPadState::new();
        let (l, h) = powerpad_read8(&mut p);
        assert_eq!(l, [false; 8], "no L bits with nothing pressed");
        assert_eq!(h, [false, false, false, false, true, true, true, true]);
    }

    #[test]
    fn powerpad_button_maps_to_expected_serial_position() {
        // Mat button "1" (index 0) is bit 1 of L -> appears on the 2nd read.
        let mut p = PowerPadState::new();
        p.set(1 << 0);
        let (l, _h) = powerpad_read8(&mut p);
        assert_eq!(l, [false, true, false, false, false, false, false, false]);

        // Mat button "2" (index 1) is bit 0 of L -> appears on the 1st read.
        let mut p = PowerPadState::new();
        p.set(1 << 1);
        let (l, _h) = powerpad_read8(&mut p);
        assert_eq!(l, [true, false, false, false, false, false, false, false]);

        // Mat button "4" (index 3) is bit 0 of H -> 1st read on bit 4.
        let mut p = PowerPadState::new();
        p.set(1 << 3);
        let (_l, h) = powerpad_read8(&mut p);
        assert_eq!(h, [true, false, false, false, true, true, true, true]);
    }

    #[test]
    fn powerpad_strobe_high_reloads_each_read() {
        // While strobe is high, every read re-latches, so the first serial bit
        // is returned repeatedly (standard controller strobe semantics).
        let mut p = PowerPadState::new();
        p.set(1 << 1); // button "2" -> L bit 0 (1st-read position).
        p.write_strobe(1); // strobe held high
        for _ in 0..5 {
            assert_eq!(p.read() & (1 << 3), 1 << 3, "strobe-high repeats bit 0");
        }
    }

    #[test]
    fn powerpad_save_state_round_trip() {
        let mut p = PowerPadState::new();
        p.set(0b1010_0101_0011);
        p.write_strobe(1);
        p.write_strobe(0);
        let _ = p.read(); // advance the registers
        let restored = PowerPadState::from_parts(
            p.buttons_raw(),
            p.shift_l_raw(),
            p.shift_h_raw(),
            p.strobe_raw(),
        );
        assert_eq!(restored.peek(), p.peek());
        assert_eq!(restored.buttons_raw(), 0b1010_0101_0011);
    }

    #[test]
    fn powerpad_masks_to_12_bits() {
        // Bits above 11 are ignored (the mat has 12 buttons).
        let mut p = PowerPadState::new();
        p.set(0xFFFF);
        assert_eq!(p.buttons_raw(), 0x0FFF);
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

    /// Shift out `n` D0 bits (MSb-first), returning them packed into a u64 in
    /// read order (first bit = most significant of the returned `n`-bit value).
    fn mouse_read_bits(m: &mut SnesMouseState, n: usize) -> u64 {
        m.write_strobe(1);
        m.write_strobe(0);
        let mut acc = 0u64;
        for _ in 0..n {
            acc = (acc << 1) | u64::from(m.read() & 1);
        }
        acc
    }

    #[test]
    fn snes_mouse_signature_nibble_is_0b0001() {
        let mut m = SnesMouseState::new();
        // First 4 bits are the device-id signature nibble 0b0001.
        let bits = mouse_read_bits(&mut m, 4);
        assert_eq!(bits, 0b0001, "signature nibble must be 0b0001");
    }

    #[test]
    fn snes_mouse_full_report_encodes_buttons_and_movement() {
        let mut m = SnesMouseState::new();
        // dx = +5, dy = -3, left pressed, sensitivity = 2 (high).
        m.set(5, -3, true, false, 2);
        let word = mouse_read_bits(&mut m, 32);
        // Reconstruct the full 32-bit word and check each field.
        assert_eq!((word >> 28) & 0x0F, 0b0001, "signature");
        assert_eq!((word >> 24) & 0b11, 2, "sensitivity");
        assert_eq!((word >> 23) & 1, 1, "left button");
        assert_eq!((word >> 22) & 1, 0, "right button");
        // Y field (bits 15..8): sign=1 (negative), magnitude 3.
        let y = (word >> 8) & 0xFF;
        assert_eq!((y >> 7) & 1, 1, "Y sign negative");
        assert_eq!(y & 0x7F, 3, "Y magnitude");
        // X field (bits 7..0): sign=0 (positive), magnitude 5.
        let x = word & 0xFF;
        assert_eq!((x >> 7) & 1, 0, "X sign positive");
        assert_eq!(x & 0x7F, 5, "X magnitude");
    }

    #[test]
    fn snes_mouse_idles_high_after_32_bits() {
        let mut m = SnesMouseState::new();
        m.write_strobe(1);
        m.write_strobe(0);
        for _ in 0..32 {
            let _ = m.read();
        }
        for _ in 0..4 {
            assert_eq!(m.read() & 1, 1, "serial line idles high after the report");
        }
    }

    #[test]
    fn snes_mouse_clamps_movement_to_127() {
        let mut m = SnesMouseState::new();
        m.set(1000, -1000, false, false, 0);
        let word = mouse_read_bits(&mut m, 32);
        assert_eq!(word & 0x7F, 127, "X magnitude clamps to 127");
        assert_eq!((word >> 8) & 0x7F, 127, "Y magnitude clamps to 127");
    }

    #[test]
    fn snes_mouse_enc_axis_handles_i16_extremes_without_panic() {
        // Regression: `enc_axis(i16::MIN)` must not panic on the `-v` overflow
        // (`-(-32768)` is unrepresentable as i16). Both extremes encode sanely:
        // sign bit set/clear and magnitude clamped to the 7-bit max of 127.
        let mut m = SnesMouseState::new();
        m.set(i16::MIN, i16::MAX, false, false, 0);
        let word = mouse_read_bits(&mut m, 32);
        // X = i16::MIN: negative -> sign bit (bit 7) set, magnitude clamped 127.
        assert_eq!(word & 0x80, 0x80, "i16::MIN encodes as negative");
        assert_eq!(word & 0x7F, 127, "i16::MIN magnitude clamps to 127");
        // Y = i16::MAX: positive -> sign bit clear, magnitude clamped 127.
        assert_eq!((word >> 8) & 0x80, 0, "i16::MAX encodes as positive");
        assert_eq!((word >> 8) & 0x7F, 127, "i16::MAX magnitude clamps to 127");
    }

    #[test]
    fn snes_mouse_strobe_high_repeats_signature_bit() {
        let mut m = SnesMouseState::new();
        m.write_strobe(1); // held high
        for _ in 0..5 {
            // Signature MSb (bit 31 of 0b0001 << 28) is 0; repeats while strobed.
            assert_eq!(m.read() & 1, 0, "strobe-high repeats bit 31");
        }
    }

    #[test]
    fn family_keyboard_disabled_reads_all_high() {
        let k = FamilyKeyboardState::new();
        // Not enabled (bit 2 unset): key switches all read high (bits 4..1 set).
        assert_eq!(
            k.read(),
            0b0001_1110,
            "disabled matrix -> no keys (bits 4..1=1)"
        );
    }

    #[test]
    fn family_keyboard_pressed_key_reads_active_low() {
        let mut k = FamilyKeyboardState::new();
        // Press key at row 0, column-half 0, switch 0 (linear index 0).
        k.set_key(0, true);
        // Enable matrix (bit2), select column-half 0 (bit0=0), clock low resets row to 0.
        k.write_strobe(0b0000_0100);
        let r = k.read();
        // The pressed switch (bit 0 of the nibble) appears active-low on bit 1.
        assert_eq!(r & (1 << 1), 0, "pressed key reads 0 (active-low) on bit 1");
        // The other three switches are not pressed -> read 1.
        assert_eq!(r & (1 << 2), 1 << 2);
        assert_eq!(r & (1 << 3), 1 << 3);
        assert_eq!(r & (1 << 4), 1 << 4);
    }

    #[test]
    fn family_keyboard_column_select_picks_high_nibble() {
        let mut k = FamilyKeyboardState::new();
        // Key at row 0, column-half 1, switch 0 = linear index 4 (row*8 + 4).
        k.set_key(4, true);
        // Enable + select column-half 1 (bit0=1), clock low (resets row to 0).
        k.write_strobe(0b0000_0101);
        let r = k.read();
        assert_eq!(
            r & (1 << 1),
            0,
            "column-half-1 key reads active-low on bit 1"
        );
        // Selecting column-half 0 instead shows nothing pressed there.
        k.write_strobe(0b0000_0100);
        assert_eq!(k.read() & (1 << 1), 1 << 1, "column-half 0 has no key here");
    }

    #[test]
    fn family_keyboard_clock_edge_advances_row() {
        let mut k = FamilyKeyboardState::new();
        // Press a key on row 1, column-half 0, switch 0 = linear index 8.
        k.set_key(8, true);
        // Enable + clock low -> row 0.
        k.write_strobe(0b0000_0100);
        assert_eq!(k.read() & (1 << 1), 1 << 1, "row 0 has no key");
        // Rising clock edge (bit1 0->1) advances to row 1.
        k.write_strobe(0b0000_0110);
        assert_eq!(
            k.read() & (1 << 1),
            0,
            "row 1 key now selected (active-low)"
        );
    }

    #[test]
    fn family_keyboard_save_state_round_trip() {
        let mut k = FamilyKeyboardState::new();
        k.set_key(8, true);
        k.set_key(40, true);
        k.write_strobe(0b0000_0110);
        let restored = FamilyKeyboardState::from_parts(
            k.keys_raw(),
            k.row_raw(),
            k.column_raw(),
            k.enabled_raw(),
            k.clock_raw(),
        );
        assert_eq!(restored.read(), k.read());
        assert_eq!(restored.keys_raw(), k.keys_raw());
    }

    #[test]
    fn family_keyboard_from_parts_clamps_out_of_range_row() {
        // A corrupt/malicious save-state must not be able to drive a row value
        // that would index `self.keys[row]` out of bounds in `read()`.
        let keys = [0u8; FAMILY_KEYBOARD_ROWS];
        let restored = FamilyKeyboardState::from_parts(keys, 250, false, true, false);
        assert!(
            (restored.row_raw() as usize) < FAMILY_KEYBOARD_ROWS,
            "out-of-range row saturated to the matrix bound"
        );
        // Must not panic: enabled matrix indexes keys[row] in read().
        let _ = restored.read();
    }

    #[test]
    fn family_keyboard_set_key_out_of_range_is_noop() {
        let mut k = FamilyKeyboardState::new();
        k.set_key(FAMILY_KEYBOARD_KEYS, true); // index == 72, out of range
        k.set_key(1000, true);
        assert_eq!(k.keys_raw(), [0; FAMILY_KEYBOARD_ROWS]);
    }

    // --- v1.3.0 Workstream F1 — niche peripheral aliases + Hyper Shots ---

    #[test]
    fn family_trainer_reuses_power_pad_scan() {
        // The Family Trainer is layout-equivalent to the Power Pad: an identical
        // PowerPadState must produce an identical serial readout through both
        // InputDevice variants.
        let mut pad = InputDevice::PowerPad(PowerPadState::new());
        let mut mat = InputDevice::FamilyTrainer(PowerPadState::new());
        if let (InputDevice::PowerPad(p), InputDevice::FamilyTrainer(m)) = (&mut pad, &mut mat) {
            p.set(0b1010_0101_0011);
            m.set(0b1010_0101_0011);
        }
        pad.write_strobe(1);
        pad.write_strobe(0);
        mat.write_strobe(1);
        mat.write_strobe(0);
        for i in 0..8 {
            assert_eq!(pad.read(), mat.read(), "read {i}: trainer == power pad");
        }
    }

    #[test]
    fn subor_keyboard_reuses_family_keyboard_scan() {
        // The Subor keyboard reuses the Family BASIC keyboard matrix scan; the
        // same key state must read identically through both variants.
        let mut fam = FamilyKeyboardState::new();
        let mut sub = FamilyKeyboardState::new();
        fam.set_key(8, true);
        sub.set_key(8, true);
        let mut famd = InputDevice::FamilyKeyboard(fam);
        let mut subd = InputDevice::SuborKeyboard(sub);
        // Enable + clock low (row 0), then rising edge -> row 1.
        for v in [0b0000_0100u8, 0b0000_0110] {
            famd.write_strobe(v);
            subd.write_strobe(v);
        }
        assert_eq!(famd.read(), subd.read(), "subor == family keyboard read");
        assert_eq!(famd.peek(), subd.peek());
    }

    #[test]
    fn konami_hyper_shot_buttons_on_expected_bits() {
        let mut h = KonamiHyperShotState::new();
        // P1 Run (bit0) -> read bit 1; P2 Jump (bit3) -> read bit 4.
        h.set(0b1001);
        h.write_strobe(0); // enable both players (active-low)
        let r = h.read();
        assert_eq!(r & (1 << 1), 1 << 1, "P1 Run on bit 1");
        assert_eq!(r & (1 << 4), 1 << 4, "P2 Jump on bit 4");
        assert_eq!(r & (1 << 2), 0, "P1 Jump not pressed");
        assert_eq!(r & (1 << 3), 0, "P2 Run not pressed");
    }

    #[test]
    fn konami_hyper_shot_disabled_player_reads_zero() {
        let mut h = KonamiHyperShotState::new();
        h.set(0b1111); // all four buttons held
        // Disable P1 (bit 1 set), enable P2 (bit 2 clear).
        h.write_strobe(0b0000_0010);
        let r = h.read();
        assert_eq!(r & (1 << 1), 0, "disabled P1 Run reads 0");
        assert_eq!(r & (1 << 2), 0, "disabled P1 Jump reads 0");
        assert_eq!(r & (1 << 3), 1 << 3, "enabled P2 Run reads pressed");
        assert_eq!(r & (1 << 4), 1 << 4, "enabled P2 Jump reads pressed");
    }

    #[test]
    fn konami_hyper_shot_save_state_round_trip() {
        let mut h = KonamiHyperShotState::new();
        h.set(0b0110);
        h.write_strobe(0b0000_0100); // disable P2
        let r = KonamiHyperShotState::from_parts(
            h.buttons_raw(),
            h.p1_enabled_raw(),
            h.p2_enabled_raw(),
        );
        assert_eq!(r.peek(), h.peek());
        assert_eq!(r.buttons_raw(), 0b0110);
    }

    #[test]
    fn bandai_hyper_shot_select_picks_sensor_group() {
        let mut b = BandaiHyperShotState::new();
        // Group A=0 = bits 0..=3 (Left Hook = bit 0), A=1 = bits 4..=7.
        b.set(0b0001_0001); // Left Hook (group0) + Left Jab (group1)
        b.write_strobe(0); // A=0 group
        assert_eq!(b.read() & (1 << 1), 1 << 1, "group0 sensor 0 on bit 1");
        b.write_strobe(0b0000_0010); // A=1 group
        assert_eq!(b.read() & (1 << 1), 1 << 1, "group1 sensor 0 on bit 1");
        // A sensor only in group0 must vanish when the A=1 group is selected.
        let mut b2 = BandaiHyperShotState::new();
        b2.set(0b0000_1000); // only group0 bit 3 (Right Hook)
        b2.write_strobe(0b0000_0010); // select A=1
        assert_eq!(b2.read() & 0b1_1110, 0, "group0-only sensor absent in A=1");
    }

    #[test]
    fn bandai_hyper_shot_save_state_round_trip() {
        let mut b = BandaiHyperShotState::new();
        b.set(0b1100_0011);
        b.write_strobe(0b0000_0010);
        let r = BandaiHyperShotState::from_parts(b.sensors_raw(), b.select_raw());
        assert_eq!(r.peek(), b.peek());
        assert_eq!(r.sensors_raw(), 0b1100_0011);
        assert!(r.select_raw());
    }

    #[test]
    fn hyper_shots_dispatch_through_input_device() {
        let mut k = InputDevice::KonamiHyperShot(KonamiHyperShotState::new());
        k.write_strobe(0);
        let _ = k.read();
        let _ = k.peek();
        let mut bd = InputDevice::BandaiHyperShot(BandaiHyperShotState::new());
        bd.write_strobe(0);
        let _ = bd.read();
        let _ = bd.peek();
    }
}

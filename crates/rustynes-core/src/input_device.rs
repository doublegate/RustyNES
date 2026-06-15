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
}

impl InputDevice {
    /// Forward a `$4016` strobe write to the device (only the Vaus latches on
    /// it; the Zapper ignores it).
    pub const fn write_strobe(&mut self, value: u8) {
        match self {
            Self::Vaus(v) => v.write_strobe(value),
            Self::PowerPad(p) => p.write_strobe(value),
            Self::SnesMouse(m) => m.write_strobe(value),
            Self::FamilyKeyboard(k) => k.write_strobe(value),
            Self::Zapper(_) => {}
        }
    }

    /// Read the device byte (already bit-positioned), advancing any internal
    /// shift register.
    pub const fn read(&mut self) -> u8 {
        match self {
            Self::Vaus(v) => v.read(),
            Self::Zapper(z) => z.read(),
            Self::PowerPad(p) => p.read(),
            Self::SnesMouse(m) => m.read(),
            Self::FamilyKeyboard(k) => k.read(),
        }
    }

    /// Side-effect-free sample of the device byte (debugger peek).
    #[must_use]
    pub const fn peek(&self) -> u8 {
        match self {
            Self::Vaus(v) => v.peek(),
            Self::Zapper(z) => z.read(),
            Self::PowerPad(p) => p.peek(),
            Self::SnesMouse(m) => m.peek(),
            Self::FamilyKeyboard(k) => k.peek(),
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
}

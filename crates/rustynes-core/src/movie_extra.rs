//! v3.5.0 (`T-MOVIE-INPUT-STREAM`, backlog CR-03) — the non-controller half
//! of a movie frame: expansion devices, the Famicom microphone, the Vs.
//! System coin and service inputs, the FDS disk, and Datach barcode scans.
//!
//! # Why this exists
//!
//! Until v3.5.0 a `.rnm` movie recorded the four controller ports and nothing
//! else (ADR 0044 listed the rest as "Neutral"), so a run of a Zapper, Vs. or
//! FDS title replayed with the trigger never pulled, no coin ever inserted and
//! the disk never swapped: it diverged on the first frame that input mattered.
//! [`ExtraInput`] is every other input [`Nes`] accepts from a host, read off
//! the machine by [`ExtraInput::capture`] and driven back by
//! [`ExtraInput::apply`]. The set was enumerated from `Nes`'s own setters,
//! not from a list of devices:
//!
//! | `Nes` setter | recorded as |
//! | --- | --- |
//! | `set_expansion_device`, `set_zapper`, `set_paddle`, `set_power_pad`, `set_family_trainer`, `set_snes_mouse`, `set_family_keyboard`, `set_subor_keyboard`, `set_konami_hyper_shot`, `set_bandai_hyper_shot` | [`ExtraInput::devices`] (per port, [`DeviceInput`]) |
//! | `set_microphone` | [`ExtraInput::microphone`] |
//! | `insert_coin`, `clear_coin` | [`ExtraInput::vs_coin`] |
//! | `set_vs_service` | [`ExtraInput::vs_service`] |
//! | `set_disk_side` | [`ExtraInput::disk_side`] |
//! | `scan_barcode` | [`ExtraInput::barcode`] |
//!
//! Not inputs, and so not here: the Vs. DIP switches, the Four Score and the
//! Vs. PPU type are configuration, recorded once in the movie's
//! [`crate::HardwareOptions`]; `set_disk_write_protected` is a property of
//! the medium the host persists, never set during play by any frontend; and
//! the Zapper's photodiode is computed from the picture, not supplied.
//! Reset and Power Cycle are still not recorded (a movie session refuses
//! them, `docs/frontend.md`).
//!
//! # Levels, not events, in memory
//!
//! Every field is the level the machine held when the frame started, which
//! is what a recorder can read back without the host's cooperation and what
//! a TAS editor can show and edit per frame. The one true event, a barcode
//! swipe, is carried as "a scan was made before this frame" and is never
//! repeated.
//!
//! On disk the levels are stored as the changes between frames (the INPUT
//! EVENTS block, [`encode_events`]), so a movie whose devices sit still costs
//! a few bytes, not a few dozen per frame.
//!
//! # `None` means "this frame does not drive these inputs"
//!
//! [`crate::FrameInput::extra`] is an `Option`. A recorded frame always
//! carries `Some`; a frame built by an importer (`.fm2`, `.bk2`, the legacy
//! containers), a TAS editor's inserted blank frame, or an input macro
//! carries `None`, and playback leaves the machine's non-controller inputs as
//! they are. That keeps every pre-v3.5.0 producer meaning what it meant
//! (controllers only), and stops a blank frame from ejecting the FDS disk or
//! unplugging the Zapper: the neutral value of [`ExtraInput`] says "no device,
//! disk ejected", which is a real input, not an absence of one.

use alloc::vec::Vec;

use crate::input_device::InputDevice;
use crate::nes::Nes;
use crate::save_state::{BinReader, BinWriter};
use rustynes_mappers::Barcode;

/// What one controller port's expansion device is doing in a movie frame.
///
/// One variant per [`InputDevice`] kind, holding exactly the fields a host
/// sets through that device's `Nes` setter -- never the device's internal
/// shift registers or strobe latch, which the emulation derives. A replay
/// that sets these fields at the same point the host did reproduces the
/// device's reads.
///
/// `#[non_exhaustive]`: a device added later is an additive change.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum DeviceInput {
    /// No expansion device: the port reads its standard controller (or the
    /// Four Score).
    #[default]
    None,
    /// NES Zapper: aim point and trigger ([`Nes::set_zapper`]). The light
    /// sense is computed from the picture, not recorded.
    Zapper {
        /// Aim X, 0..256 (out of range = aimed off-screen).
        x: u16,
        /// Aim Y, 0..240 (out of range = aimed off-screen).
        y: u16,
        /// Trigger pulled.
        trigger: bool,
    },
    /// Arkanoid "Vaus" paddle ([`Nes::set_paddle`]).
    Vaus {
        /// Raw potentiometer position.
        position: u8,
        /// Fire button held.
        fire: bool,
    },
    /// NES Power Pad mat: the 12-button mask ([`Nes::set_power_pad`]).
    PowerPad(u16),
    /// Bandai Family Trainer mat: the 12-button mask
    /// ([`Nes::set_family_trainer`]).
    FamilyTrainer(u16),
    /// SNES-style mouse ([`Nes::set_snes_mouse`]).
    SnesMouse {
        /// Delta X as the host set it (clamped by the device on latch).
        dx: i16,
        /// Delta Y as the host set it.
        dy: i16,
        /// Left button held.
        left: bool,
        /// Right button held.
        right: bool,
        /// Reported sensitivity, 0..=3 (the device keeps two bits).
        sensitivity: u8,
    },
    /// Family BASIC keyboard: one byte per matrix row
    /// ([`Nes::set_family_keyboard`]).
    FamilyKeyboard([u8; 9]),
    /// Subor keyboard: one byte per matrix row ([`Nes::set_subor_keyboard`]).
    SuborKeyboard([u8; 9]),
    /// Konami Hyper Shot: the 4-button mask ([`Nes::set_konami_hyper_shot`]).
    KonamiHyperShot(u8),
    /// Bandai Hyper Shot: the 8-sensor mask ([`Nes::set_bandai_hyper_shot`]).
    BandaiHyperShot(u8),
}

/// The largest [`DeviceInput`] payload, in bytes (a keyboard's 9 rows).
const DEVICE_PAYLOAD_MAX: usize = 9;

impl DeviceInput {
    /// Read the host-set fields of the device attached to a port.
    #[must_use]
    pub const fn capture(device: &Option<InputDevice>) -> Self {
        match device {
            None => Self::None,
            Some(InputDevice::Zapper(z)) => Self::Zapper {
                x: z.x,
                y: z.y,
                trigger: z.trigger,
            },
            Some(InputDevice::Vaus(v)) => Self::Vaus {
                position: v.position,
                fire: v.fire,
            },
            Some(InputDevice::PowerPad(p)) => Self::PowerPad(p.buttons),
            Some(InputDevice::FamilyTrainer(p)) => Self::FamilyTrainer(p.buttons),
            Some(InputDevice::SnesMouse(m)) => Self::SnesMouse {
                dx: m.dx,
                dy: m.dy,
                left: m.left,
                right: m.right,
                sensitivity: m.sensitivity,
            },
            Some(InputDevice::FamilyKeyboard(k)) => Self::FamilyKeyboard(k.keys),
            Some(InputDevice::SuborKeyboard(k)) => Self::SuborKeyboard(k.keys),
            Some(InputDevice::KonamiHyperShot(h)) => Self::KonamiHyperShot(h.buttons),
            Some(InputDevice::BandaiHyperShot(b)) => Self::BandaiHyperShot(b.sensors),
        }
    }

    /// Drive `port` of `nes` to this device input.
    ///
    /// A different device kind is attached fresh (exactly what the `Nes`
    /// convenience setters do when the kind is absent); the same kind keeps
    /// its internal state and only has its host-set fields written, as the
    /// host's own per-frame call did. [`Self::None`] unplugs a device.
    ///
    /// # Panics
    ///
    /// Panics if `port` is not 0 or 1.
    pub fn apply(self, nes: &mut Nes, port: usize) {
        match self {
            Self::None => {
                if nes.expansion_device(port).is_some() {
                    nes.set_expansion_device(port, None);
                }
            }
            Self::Zapper { x, y, trigger } => nes.set_zapper(port, x, y, trigger),
            Self::Vaus { position, fire } => nes.set_paddle(port, position, fire),
            Self::PowerPad(b) => nes.set_power_pad(port, b),
            Self::FamilyTrainer(b) => nes.set_family_trainer(port, b),
            Self::SnesMouse {
                dx,
                dy,
                left,
                right,
                sensitivity,
            } => nes.set_snes_mouse(port, dx, dy, left, right, sensitivity),
            Self::FamilyKeyboard(k) => nes.set_family_keyboard(port, k),
            Self::SuborKeyboard(k) => nes.set_subor_keyboard(port, k),
            Self::KonamiHyperShot(b) => nes.set_konami_hyper_shot(port, b),
            Self::BandaiHyperShot(b) => nes.set_bandai_hyper_shot(port, b),
        }
    }

    /// The on-disk device tag.
    const fn tag(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Zapper { .. } => 1,
            Self::Vaus { .. } => 2,
            Self::PowerPad(_) => 3,
            Self::FamilyTrainer(_) => 4,
            Self::SnesMouse { .. } => 5,
            Self::FamilyKeyboard(_) => 6,
            Self::SuborKeyboard(_) => 7,
            Self::KonamiHyperShot(_) => 8,
            Self::BandaiHyperShot(_) => 9,
        }
    }

    /// Encode the payload into `out`, returning its length.
    fn payload(self, out: &mut [u8; DEVICE_PAYLOAD_MAX]) -> usize {
        match self {
            Self::None => 0,
            Self::Zapper { x, y, trigger } => {
                out[..2].copy_from_slice(&x.to_le_bytes());
                out[2..4].copy_from_slice(&y.to_le_bytes());
                out[4] = u8::from(trigger);
                5
            }
            Self::Vaus { position, fire } => {
                out[0] = position;
                out[1] = u8::from(fire);
                2
            }
            // Masked as the device's setter masks them, so a value built by
            // hand encodes to one the decoder accepts (and replays the same:
            // the setter would have dropped those bits anyway).
            Self::PowerPad(b) | Self::FamilyTrainer(b) => {
                out[..2].copy_from_slice(&(b & 0x0FFF).to_le_bytes());
                2
            }
            Self::SnesMouse {
                dx,
                dy,
                left,
                right,
                sensitivity,
            } => {
                out[..2].copy_from_slice(&dx.to_le_bytes());
                out[2..4].copy_from_slice(&dy.to_le_bytes());
                out[4] = u8::from(left) | (u8::from(right) << 1);
                out[5] = sensitivity & 0b11;
                6
            }
            Self::FamilyKeyboard(k) | Self::SuborKeyboard(k) => {
                out.copy_from_slice(&k);
                9
            }
            Self::KonamiHyperShot(b) => {
                out[0] = b & 0x0F;
                1
            }
            Self::BandaiHyperShot(b) => {
                out[0] = b;
                1
            }
        }
    }

    fn write(self, w: &mut BinWriter) {
        let mut buf = [0u8; DEVICE_PAYLOAD_MAX];
        let n = self.payload(&mut buf);
        w.u8(self.tag());
        w.bytes(&buf[..n]);
    }

    /// Decode a tag and its payload. Only the values a device can actually
    /// hold are accepted (the masks its setter applies), so a decoded
    /// movie re-encodes to the same bytes.
    fn read(r: &mut BinReader<'_>) -> Result<Self, &'static str> {
        let tag = r.u8().map_err(|_| EOF)?;
        let dev = match tag {
            0 => Self::None,
            1 => Self::Zapper {
                x: r.u16().map_err(|_| EOF)?,
                y: r.u16().map_err(|_| EOF)?,
                trigger: read_flag(r)?,
            },
            2 => Self::Vaus {
                position: r.u8().map_err(|_| EOF)?,
                fire: read_flag(r)?,
            },
            3 | 4 => {
                let b = r.u16().map_err(|_| EOF)?;
                if b > 0x0FFF {
                    return Err("a mat button mask has bits above button 12");
                }
                if tag == 3 {
                    Self::PowerPad(b)
                } else {
                    Self::FamilyTrainer(b)
                }
            }
            5 => {
                let dx = r.i16().map_err(|_| EOF)?;
                let dy = r.i16().map_err(|_| EOF)?;
                let buttons = r.u8().map_err(|_| EOF)?;
                if buttons & !0b11 != 0 {
                    return Err("a mouse button byte has bits beyond left and right");
                }
                let sensitivity = r.u8().map_err(|_| EOF)?;
                if sensitivity > 3 {
                    return Err("a mouse sensitivity is above 3");
                }
                Self::SnesMouse {
                    dx,
                    dy,
                    left: buttons & 1 != 0,
                    right: buttons & 2 != 0,
                    sensitivity,
                }
            }
            6 | 7 => {
                let mut k = [0u8; 9];
                r.read_into(&mut k).map_err(|_| EOF)?;
                if tag == 6 {
                    Self::FamilyKeyboard(k)
                } else {
                    Self::SuborKeyboard(k)
                }
            }
            8 => {
                let b = r.u8().map_err(|_| EOF)?;
                if b > 0x0F {
                    return Err("a Konami Hyper Shot mask has bits above button 4");
                }
                Self::KonamiHyperShot(b)
            }
            9 => Self::BandaiHyperShot(r.u8().map_err(|_| EOF)?),
            _ => return Err("an expansion device tag this build does not know"),
        };
        Ok(dev)
    }
}

/// Every non-controller input of one movie frame. See the module docs.
///
/// [`Default`] is the neutral machine: no device on either port, microphone
/// released, no coin, service released, **disk ejected**, no scan. It is the
/// baseline the on-disk changes are taken against, not "nothing recorded"
/// (that is [`crate::FrameInput::extra`] being `None`).
///
/// `#[non_exhaustive]`: build one with [`Self::capture`] or [`Default`] and
/// set fields.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub struct ExtraInput {
    /// The expansion device on port 0 (`$4016`) and port 1 (`$4017`).
    pub devices: [DeviceInput; 2],
    /// Famicom microphone asserted (`$4016` bit 2).
    pub microphone: bool,
    /// Vs. System coin acceptors latched: bit 0 = #1, bit 1 = #2.
    pub vs_coin: u8,
    /// Vs. System service button held.
    pub vs_service: bool,
    /// The inserted FDS disk side (0-based), or `None` when ejected -- and
    /// always `None` on a cartridge, which has no disk. Side 255 cannot be
    /// stored (its byte means "ejected"); no FDS image comes near it.
    pub disk_side: Option<u8>,
    /// A Datach barcode swiped just before this frame (an event: set on the
    /// one frame the scan was made before, `None` on every other).
    pub barcode: Option<Barcode>,
}

impl ExtraInput {
    /// Read every non-controller input `nes` holds now. Called by
    /// [`crate::FrameInput::held_on`], after the host's input calls and
    /// before the frame.
    ///
    /// A disk side beyond 254 cannot be stored; no FDS image has that many
    /// (a side is 65,500 bytes), and such a side records as ejected.
    #[must_use]
    pub fn capture(nes: &Nes) -> Self {
        Self {
            devices: [
                DeviceInput::capture(nes.expansion_device(0)),
                DeviceInput::capture(nes.expansion_device(1)),
            ],
            microphone: nes.microphone(),
            vs_coin: nes.vs_coin(),
            vs_service: nes.vs_service(),
            disk_side: nes
                .inserted_disk_side()
                .and_then(|s| u8::try_from(s).ok())
                .filter(|&s| s != DISK_EJECTED),
            barcode: nes.barcode_this_frame(),
        }
    }

    /// Drive `nes` to these inputs, before the frame runs.
    ///
    /// Levels the host sets every frame (devices, microphone, coin, service)
    /// are written every frame, as the host wrote them. The disk is changed
    /// only when it differs, because inserting a side is an act with
    /// consequences (it resets the head and opens the drive's not-ready
    /// window), and a host only does it once. A barcode is swiped when
    /// present.
    ///
    /// A scan the machine cannot take (no reader) is ignored: the movie's
    /// board check ([`crate::Movie::seek_to_start`]) already refused a
    /// machine without the cartridge it was recorded on.
    pub fn apply(&self, nes: &mut Nes) {
        self.devices[0].apply(nes, 0);
        self.devices[1].apply(nes, 1);
        nes.set_microphone(self.microphone);
        if nes.vs_coin() != self.vs_coin {
            nes.clear_coin();
            for acceptor in 0..2u8 {
                if self.vs_coin & (1 << acceptor) != 0 {
                    nes.insert_coin(acceptor);
                }
            }
        }
        nes.set_vs_service(self.vs_service);
        let want = self.disk_side.map(usize::from);
        if nes.inserted_disk_side() != want {
            nes.set_disk_side(want);
        }
        if let Some(code) = &self.barcode {
            let _ = nes.scan_barcode_code(code);
        }
    }

    /// This frame's inputs with the barcode event removed: what the next
    /// frame starts from when it records no change.
    #[must_use]
    pub const fn held(mut self) -> Self {
        self.barcode = None;
        self
    }
}

/// The disk byte meaning "ejected".
const DISK_EJECTED: u8 = 0xFF;

/// The decode error for a truncated INPUT EVENTS block.
const EOF: &str = "the input events end mid-event";

fn read_flag(r: &mut BinReader<'_>) -> Result<bool, &'static str> {
    match r.u8().map_err(|_| EOF)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err("a boolean input is neither 0 nor 1"),
    }
}

// ---------------------------------------------------------------------------
// The INPUT EVENTS block (`.rnm` format 7)
// ---------------------------------------------------------------------------

/// From this frame on, frames carry no [`ExtraInput`] (`extra` is `None`).
const EV_EXTRA_OFF: u8 = 0x00;
/// From this frame on, frames carry an [`ExtraInput`], starting from the
/// neutral value; the changes that follow in the same frame set it.
const EV_EXTRA_ON: u8 = 0x01;
/// `port: u8`, then a [`DeviceInput`] (tag + payload).
const EV_DEVICE: u8 = 0x02;
/// `asserted: u8` (0 or 1).
const EV_MICROPHONE: u8 = 0x03;
/// `coin bits: u8` (0..=3).
const EV_VS_COIN: u8 = 0x04;
/// `held: u8` (0 or 1).
const EV_VS_SERVICE: u8 = 0x05;
/// `side: u8`, `0xFF` = ejected.
const EV_DISK: u8 = 0x06;
/// `digit count: u8` (8 or 13), then that many digits 0..=9.
const EV_BARCODE: u8 = 0x07;

/// Smallest event on disk: frame (4) + kind (1), for the allocation bound.
const MIN_EVENT_LEN: usize = 5;

/// Encode the non-controller half of `frames` as the INPUT EVENTS block body:
/// `u32` event count, then each event as `u32` frame index, `u8` kind and a
/// kind-specific payload, in frame order and, within a frame, in kind (then
/// port) order.
///
/// An event is written only where a frame differs from the one before it
/// (the first frame from "nothing recorded"), plus every barcode scan.
pub fn encode_events<'a>(frames: impl Iterator<Item = Option<&'a ExtraInput>>) -> Vec<u8> {
    let mut body = BinWriter::new();
    let mut count = 0u32;
    let mut prev: Option<ExtraInput> = None;
    for (i, cur) in frames.enumerate() {
        let frame = u32::try_from(i).expect("frame count exceeds u32");
        let mut ev = |kind: u8, body: &mut BinWriter| {
            body.u32(frame);
            body.u8(kind);
            count += 1;
        };
        match (prev, cur) {
            (None, None) => {}
            (Some(_), None) => ev(EV_EXTRA_OFF, &mut body),
            (p, Some(c)) => {
                let base = p.unwrap_or_else(|| {
                    ev(EV_EXTRA_ON, &mut body);
                    ExtraInput::default()
                });
                for port in 0..2u8 {
                    let p = usize::from(port);
                    if c.devices[p] != base.devices[p] {
                        ev(EV_DEVICE, &mut body);
                        body.u8(port);
                        c.devices[p].write(&mut body);
                    }
                }
                if c.microphone != base.microphone {
                    ev(EV_MICROPHONE, &mut body);
                    body.u8(u8::from(c.microphone));
                }
                // Masked to the two acceptors `insert_coin` can latch.
                if c.vs_coin & 0b11 != base.vs_coin & 0b11 {
                    ev(EV_VS_COIN, &mut body);
                    body.u8(c.vs_coin & 0b11);
                }
                if c.vs_service != base.vs_service {
                    ev(EV_VS_SERVICE, &mut body);
                    body.u8(u8::from(c.vs_service));
                }
                if c.disk_side != base.disk_side {
                    ev(EV_DISK, &mut body);
                    body.u8(c.disk_side.unwrap_or(DISK_EJECTED));
                }
                if let Some(code) = &c.barcode {
                    ev(EV_BARCODE, &mut body);
                    let digits = code.digits();
                    body.u8(digit_count(digits));
                    body.bytes(digits);
                }
            }
        }
        prev = cur.map(|c| c.held());
    }
    let mut out = BinWriter::with_capacity(4 + body.len());
    out.u32(count);
    out.bytes(&body.into_vec());
    out.into_vec()
}

/// Decode an INPUT EVENTS block body into one `extra` value per frame.
///
/// Strict, so that every block this accepts is one [`encode_events`] could
/// have written (decode then encode returns the same bytes): events must be
/// in increasing (frame, kind, port) order and inside the movie; a field
/// event must change the value it sets; the presence events must actually
/// switch presence; and every value must be one the input can hold.
///
/// # Errors
///
/// A static description of the first violation, wrapped by the caller in
/// [`crate::MovieError::BadInputEvents`].
pub fn decode_events(
    block: &[u8],
    frame_count: usize,
) -> Result<Vec<Option<ExtraInput>>, &'static str> {
    let mut r = BinReader::new(block);
    let declared = r.u32().map_err(|_| EOF)? as usize;
    // SECURITY: `declared` is untrusted; bound any reservation by what the
    // block could hold (the `frame_count` reservation is bounded by the
    // caller, which has already read that many frame records).
    let mut events = Vec::with_capacity(declared.min(r.remaining() / MIN_EVENT_LEN));
    let mut last_key: Option<(usize, u8, u8)> = None;
    for _ in 0..declared {
        let (frame, kind, ev) = read_event(&mut r, frame_count)?;
        // Checked here, in file order, so the replay loop below can rely on
        // the list being sorted and consume every event.
        let port = if let Event::Device(p, _) = ev { p } else { 0 };
        let key = (frame, kind, port);
        if last_key.is_some_and(|k| k >= key) {
            return Err("input events are out of order or repeated");
        }
        last_key = Some(key);
        events.push((frame, ev));
    }
    if r.remaining() != 0 {
        return Err("unexpected bytes after the input events");
    }

    let mut out = Vec::with_capacity(frame_count);
    let mut cur: Option<ExtraInput> = None;
    let mut pending = events.into_iter().peekable();
    for frame in 0..frame_count {
        let mut presence_changed = false;
        while let Some((_, ev)) = pending.next_if(|&(f, _)| f == frame) {
            if let Event::Presence(on) = ev {
                if presence_changed || cur.is_some() == on {
                    return Err("an input presence event does not change presence");
                }
                presence_changed = true;
                cur = on.then(ExtraInput::default);
                continue;
            }
            let Some(state) = cur.as_mut() else {
                return Err("an input event arrives while no input is recorded");
            };
            if !apply_event(state, ev) {
                return Err("an input event repeats the value already held");
            }
        }
        out.push(cur);
        cur = cur.map(ExtraInput::held);
    }
    // Every event lies before `frame_count` and the list is sorted, so the
    // loop above consumed them all; checked anyway, because silently dropping
    // an input is the one outcome this block exists to prevent.
    if pending.next().is_some() {
        return Err("an input event was not reached");
    }
    Ok(out)
}

/// Read one event: frame index, kind and payload, validated.
fn read_event(
    r: &mut BinReader<'_>,
    frame_count: usize,
) -> Result<(usize, u8, Event), &'static str> {
    let frame = r.u32().map_err(|_| EOF)? as usize;
    if frame >= frame_count {
        return Err("an input event lies past the last frame");
    }
    let kind = r.u8().map_err(|_| EOF)?;
    let ev = match kind {
        EV_EXTRA_OFF | EV_EXTRA_ON => Event::Presence(kind == EV_EXTRA_ON),
        EV_DEVICE => {
            let port = r.u8().map_err(|_| EOF)?;
            if port > 1 {
                return Err("an expansion device event names a port other than 0 or 1");
            }
            Event::Device(port, DeviceInput::read(r)?)
        }
        EV_MICROPHONE => Event::Microphone(read_flag(r)?),
        EV_VS_COIN => {
            let bits = r.u8().map_err(|_| EOF)?;
            if bits > 0b11 {
                return Err("a Vs. coin event sets acceptors other than #1 and #2");
            }
            Event::Coin(bits)
        }
        EV_VS_SERVICE => Event::Service(read_flag(r)?),
        EV_DISK => {
            let side = r.u8().map_err(|_| EOF)?;
            Event::Disk((side != DISK_EJECTED).then_some(side))
        }
        EV_BARCODE => Event::Barcode(read_barcode(r)?),
        _ => return Err("an input event kind this build does not know"),
    };
    Ok((frame, kind, ev))
}

/// Read a barcode event's payload: a digit count (8 or 13) and the digits,
/// validated as [`Barcode::parse`] validates a typed code.
fn read_barcode(r: &mut BinReader<'_>) -> Result<Barcode, &'static str> {
    let len = usize::from(r.u8().map_err(|_| EOF)?);
    if len != 8 && len != 13 {
        return Err("a barcode event is neither 8 nor 13 digits");
    }
    let digits = r.take(len).map_err(|_| EOF)?;
    let mut text = [0u8; 13];
    for (t, &d) in text.iter_mut().zip(digits) {
        if d > 9 {
            return Err("a barcode event holds a digit above 9");
        }
        *t = b'0' + d;
    }
    let text = core::str::from_utf8(&text[..len]).map_err(|_| EOF)?;
    Barcode::parse(text).map_err(|_| "a barcode event fails its check digit")
}

/// Apply one field event to `state`, reporting whether it changed anything
/// (a barcode, being an event, always does).
fn apply_event(state: &mut ExtraInput, ev: Event) -> bool {
    match ev {
        Event::Device(p, d) => replace(&mut state.devices[usize::from(p)], d),
        Event::Microphone(v) => replace(&mut state.microphone, v),
        Event::Coin(v) => replace(&mut state.vs_coin, v),
        Event::Service(v) => replace(&mut state.vs_service, v),
        Event::Disk(v) => replace(&mut state.disk_side, v),
        Event::Barcode(code) => {
            state.barcode = Some(code);
            true
        }
        // The caller handles presence before calling this.
        Event::Presence(_) => false,
    }
}

/// A validated barcode's digit count, 8 or 13.
fn digit_count(digits: &[u8]) -> u8 {
    u8::try_from(digits.len()).unwrap_or(u8::MAX)
}

/// Set `slot` to `value`, reporting whether it changed.
fn replace<T: PartialEq>(slot: &mut T, value: T) -> bool {
    if *slot == value {
        false
    } else {
        *slot = value;
        true
    }
}

#[derive(Clone, Copy)]
enum Event {
    Presence(bool),
    Device(u8, DeviceInput),
    Microphone(bool),
    Coin(u8),
    Service(bool),
    Disk(Option<u8>),
    Barcode(Barcode),
}

/// Bytes [`attestation_bytes`] produces per frame.
pub const ATTESTATION_EXTRA_LEN: usize = 1 + 2 * (1 + DEVICE_PAYLOAD_MAX) + 4 + 1 + 13;

/// A fixed-size, canonical encoding of a frame's `extra` for the replay
/// attestation hash, so a tampered non-controller input is detected the way
/// a tampered button is.
pub fn attestation_bytes(extra: Option<&ExtraInput>) -> [u8; ATTESTATION_EXTRA_LEN] {
    let mut out = [0u8; ATTESTATION_EXTRA_LEN];
    let Some(e) = extra else {
        return out;
    };
    out[0] = 1;
    let mut at = 1;
    for dev in e.devices {
        out[at] = dev.tag();
        let mut buf = [0u8; DEVICE_PAYLOAD_MAX];
        let n = dev.payload(&mut buf);
        out[at + 1..at + 1 + n].copy_from_slice(&buf[..n]);
        at += 1 + DEVICE_PAYLOAD_MAX;
    }
    out[at] = u8::from(e.microphone);
    out[at + 1] = e.vs_coin;
    out[at + 2] = u8::from(e.vs_service);
    out[at + 3] = e.disk_side.unwrap_or(DISK_EJECTED);
    at += 4;
    if let Some(code) = &e.barcode {
        let d = code.digits();
        out[at] = digit_count(d);
        out[at + 1..at + 1 + d.len()].copy_from_slice(d);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn zapper(x: u16, trigger: bool) -> DeviceInput {
        DeviceInput::Zapper { x, y: 7, trigger }
    }

    /// A mixed stream: nothing recorded, then recorded inputs changing in
    /// every field, then nothing again, then recorded from neutral.
    fn mixed() -> Vec<Option<ExtraInput>> {
        let mut a = ExtraInput::default();
        a.devices[1] = zapper(3, false);
        a.disk_side = Some(0);
        let mut b = a;
        b.devices[1] = zapper(4, true);
        b.devices[0] = DeviceInput::SnesMouse {
            dx: -3,
            dy: 9,
            left: true,
            right: false,
            sensitivity: 2,
        };
        b.microphone = true;
        b.vs_coin = 0b10;
        b.vs_service = true;
        b.disk_side = None;
        b.barcode = Some(Barcode::parse("4901234567894").unwrap());
        let mut c = b.held();
        c.devices[0] = DeviceInput::FamilyKeyboard([9; 9]);
        vec![
            None,
            Some(a),
            Some(a),
            Some(b),
            Some(c),
            None,
            Some(ExtraInput::default()),
        ]
    }

    #[test]
    fn a_mixed_stream_round_trips_exactly() {
        let frames = mixed();
        let block = encode_events(frames.iter().map(Option::as_ref));
        let back = decode_events(&block, frames.len()).expect("decodes");
        assert_eq!(back, frames);
        // And it is canonical: re-encoding gives the same bytes.
        assert_eq!(encode_events(back.iter().map(Option::as_ref)), block);
    }

    #[test]
    fn a_barcode_is_one_frame_long() {
        let frames = mixed();
        let back = decode_events(&encode_events(frames.iter().map(Option::as_ref)), 7).unwrap();
        assert!(back[3].unwrap().barcode.is_some());
        assert!(back[4].unwrap().barcode.is_none(), "not carried forward");
    }

    #[test]
    fn controller_only_movies_cost_four_bytes() {
        let frames = vec![None; 100];
        let block = encode_events(frames.iter().map(Option::as_ref));
        assert_eq!(block, 0u32.to_le_bytes());
        assert_eq!(decode_events(&block, 100).unwrap(), frames);
    }

    /// Build a block from raw events: (frame, kind, payload).
    fn raw(events: &[(u32, u8, &[u8])]) -> Vec<u8> {
        let mut w = BinWriter::new();
        w.u32(u32::try_from(events.len()).unwrap());
        for (f, k, p) in events {
            w.u32(*f);
            w.u8(*k);
            w.bytes(p);
        }
        w.into_vec()
    }

    #[test]
    fn malformed_blocks_are_refused_with_a_reason() {
        let on = (0, EV_EXTRA_ON, &[][..]);
        let cases: &[(&str, Vec<u8>, usize)] = &[
            ("truncated count", vec![1, 0], 4),
            ("truncated event", raw(&[(0, EV_MICROPHONE, &[])]), 4),
            ("past the end", raw(&[(4, EV_EXTRA_ON, &[])]), 4),
            ("unknown kind", raw(&[on, (0, 0x7E, &[])]), 4),
            ("field while off", raw(&[(0, EV_MICROPHONE, &[1])]), 4),
            ("redundant value", raw(&[on, (0, EV_MICROPHONE, &[0])]), 4),
            ("bad bool", raw(&[on, (0, EV_MICROPHONE, &[2])]), 4),
            ("bad port", raw(&[on, (0, EV_DEVICE, &[2, 0])]), 4),
            ("bad device", raw(&[on, (0, EV_DEVICE, &[1, 42])]), 4),
            ("bad mat", raw(&[on, (0, EV_DEVICE, &[1, 3, 0, 0x10])]), 4),
            ("bad coin", raw(&[on, (0, EV_VS_COIN, &[4])]), 4),
            (
                "bad barcode length",
                raw(&[on, (0, EV_BARCODE, &[3, 1, 2, 3])]),
                4,
            ),
            (
                "bad check digit",
                // 1234567 has check digit 0, so ending in 1 is wrong.
                raw(&[on, (0, EV_BARCODE, &[8, 1, 2, 3, 4, 5, 6, 7, 1])]),
                4,
            ),
            ("off while off", raw(&[(0, EV_EXTRA_OFF, &[])]), 4),
            ("on twice", raw(&[on, (1, EV_EXTRA_ON, &[])]), 4),
            (
                "out of order",
                raw(&[on, (2, EV_MICROPHONE, &[1]), (1, EV_VS_SERVICE, &[1])]),
                4,
            ),
            (
                "repeated",
                raw(&[on, (1, EV_MICROPHONE, &[1]), (1, EV_MICROPHONE, &[0])]),
                4,
            ),
            (
                "off then on in one frame",
                raw(&[on, (1, EV_EXTRA_OFF, &[]), (1, EV_EXTRA_ON, &[])]),
                4,
            ),
        ];
        for (what, block, frames) in cases {
            assert!(
                decode_events(block, *frames).is_err(),
                "{what} must be refused"
            );
        }
        let mut trailing = raw(&[on]);
        trailing.push(0);
        assert!(decode_events(&trailing, 4).is_err(), "trailing bytes");
    }

    #[test]
    fn a_hostile_event_count_does_not_reserve_memory() {
        // Claims four billion events, holds none: refused, not allocated.
        let block = u32::MAX.to_le_bytes();
        assert!(decode_events(&block, 10).is_err());
    }

    #[test]
    fn attestation_bytes_tell_absent_from_neutral_and_see_every_field() {
        let none = attestation_bytes(None);
        let neutral = attestation_bytes(Some(&ExtraInput::default()));
        assert_ne!(none, neutral);
        let frames = mixed();
        let mut seen = alloc::collections::BTreeSet::new();
        for f in &frames {
            seen.insert(attestation_bytes(f.as_ref()));
        }
        // None (twice), a, a, b, c, neutral: five distinct records.
        assert_eq!(seen.len(), 5);
    }

    /// `FrameInput::encode_log` / `decode_log` (the `.rnmproj` version 2
    /// input log) keep players 3 and 4 and every extra input, and refuse
    /// trailing bytes.
    #[test]
    fn the_input_log_codec_keeps_everything() {
        use crate::{Buttons, FrameInput};
        let frames: Vec<FrameInput> = mixed()
            .into_iter()
            .enumerate()
            .map(|(i, extra)| {
                let b = Buttons::from_bits_truncate(u8::try_from(i * 37 % 256).unwrap());
                let mut f = FrameInput::four_players(b, b, b, b);
                f.extra = extra;
                f
            })
            .collect();
        let bytes = FrameInput::encode_log(&frames);
        assert_eq!(FrameInput::decode_log(&bytes).unwrap(), frames);
        let mut long = bytes;
        long.push(0);
        assert!(FrameInput::decode_log(&long).is_err());
        assert!(FrameInput::decode_log(&u32::MAX.to_le_bytes()).is_err());
    }
}

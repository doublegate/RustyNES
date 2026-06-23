//! Input macros / templates (v1.8.9) — reusable named per-frame input patterns
//! that feed the piano-roll's pattern-paint (the FCEUX / `BizHawk` "pattern"
//! tooling).
//!
//! A macro is a short [`FrameInput`] sequence. The editor can *extract* one from
//! a frame range ([`crate::tastudio::TasEditor::extract_macro`]) and *stamp* it
//! at the cursor ([`crate::tastudio::TasEditor::stamp_macro`]). The bank
//! round-trips through a small versioned binary blob so it can be saved / loaded.

use rustynes_core::{Buttons, FrameInput};

/// Magic prefix of a serialized macro bank (`RustyNES Macro Bank`).
const MAGIC: &[u8; 4] = b"RNMB";
/// Current bank format version.
const VERSION: u8 = 1;
/// Hard cap so a malformed blob can't ask us to pre-allocate gigabytes.
const MAX_MACROS: usize = 4096;
const MAX_FRAMES: usize = 1 << 20;

/// A named reusable input pattern (one [`FrameInput`] per frame).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputMacro {
    /// Display name.
    pub name: String,
    /// The per-frame inputs, in order.
    pub frames: Vec<FrameInput>,
}

/// A collection of input macros, persisted to a small binary file.
#[derive(Clone, Debug, Default)]
pub struct MacroBank {
    /// The macros, in display order.
    pub macros: Vec<InputMacro>,
}

impl MacroBank {
    /// Serialize to the versioned binary representation. Deterministic.
    #[must_use]
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16);
        out.extend_from_slice(MAGIC);
        out.push(VERSION);
        let count = u32::try_from(self.macros.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&count.to_le_bytes());
        for m in &self.macros {
            let name = m.name.as_bytes();
            let name_len = name.len().min(usize::from(u16::MAX));
            out.extend_from_slice(&u16::try_from(name_len).unwrap_or(u16::MAX).to_le_bytes());
            out.extend_from_slice(&name[..name_len]);
            let frames = u32::try_from(m.frames.len()).unwrap_or(u32::MAX);
            out.extend_from_slice(&frames.to_le_bytes());
            for f in &m.frames {
                out.push(f.p1.bits());
                out.push(f.p2.bits());
                out.push(f.expansion);
            }
        }
        out
    }

    /// Parse a bank from bytes. Returns `None` on a bad magic / version or any
    /// truncation — never panics on malformed input.
    #[must_use]
    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader::new(bytes);
        if r.take(4)? != MAGIC {
            return None;
        }
        if r.u8()? != VERSION {
            return None;
        }
        let count = usize::try_from(r.u32()?).ok()?;
        if count > MAX_MACROS {
            return None;
        }
        let mut macros = Vec::with_capacity(count);
        for _ in 0..count {
            let name_len = usize::from(r.u16()?);
            let name = core::str::from_utf8(r.take(name_len)?).ok()?.to_owned();
            let frame_count = usize::try_from(r.u32()?).ok()?;
            if frame_count > MAX_FRAMES {
                return None;
            }
            let mut frames = Vec::with_capacity(frame_count);
            for _ in 0..frame_count {
                let rec = r.take(3)?;
                frames.push(FrameInput {
                    p1: Buttons::from_bits_truncate(rec[0]),
                    p2: Buttons::from_bits_truncate(rec[1]),
                    expansion: rec[2],
                });
            }
            macros.push(InputMacro { name, frames });
        }
        Some(Self { macros })
    }
}

/// A minimal bounds-checked byte cursor for [`MacroBank::deserialize`].
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    const fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fi(p1: Buttons, p2: Buttons) -> FrameInput {
        FrameInput::new(p1, p2)
    }

    #[test]
    fn bank_round_trips() {
        let bank = MacroBank {
            macros: vec![
                InputMacro {
                    name: "mash A".into(),
                    frames: vec![
                        fi(Buttons::A, Buttons::empty()),
                        fi(Buttons::empty(), Buttons::empty()),
                    ],
                },
                InputMacro {
                    name: "jump".into(),
                    frames: vec![fi(Buttons::A | Buttons::RIGHT, Buttons::empty()); 4],
                },
            ],
        };
        let bytes = bank.serialize();
        let back = MacroBank::deserialize(&bytes).expect("round-trip");
        assert_eq!(back.macros, bank.macros);
    }

    #[test]
    fn empty_bank_round_trips() {
        let bytes = MacroBank::default().serialize();
        assert!(MacroBank::deserialize(&bytes).unwrap().macros.is_empty());
    }

    #[test]
    fn rejects_bad_magic_and_truncation() {
        assert!(MacroBank::deserialize(b"XXXX\x01").is_none());
        let good = MacroBank {
            macros: vec![InputMacro {
                name: "x".into(),
                frames: vec![fi(Buttons::B, Buttons::empty())],
            }],
        }
        .serialize();
        // Drop the final byte → truncated frame record → None, no panic.
        assert!(MacroBank::deserialize(&good[..good.len() - 1]).is_none());
    }
}

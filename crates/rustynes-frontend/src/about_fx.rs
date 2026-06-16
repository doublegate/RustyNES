//! About-window pointer/text input handling (native desktop only).
//!
//! Small helper that processes interaction inside the About dialog. Kept apart
//! from `ui_shell` so the window-event plumbing there stays terse. Native-only
//! and x86-64 / aarch64-only (it uses architecture-specific buffer handling); the
//! `mod` declaration in `lib.rs` is gated so every other target omits it.
//!
//! The embedded resource is password-encrypted: the key is **derived at runtime
//! from typed input** (a slow SHA-256 KDF) and is **never stored** — the binary
//! holds only ciphertext + a public salt + an integrity tag, so the resource is
//! undecryptable without the input that produced it. Decryption is accepted only
//! when the recovered plaintext matches the embedded SHA-256 tag. The SHA-256
//! primitive comes from the vetted `sha2` crate (crypto is never hand-rolled);
//! only the bulk keystream-XOR is architecture-native.
#![allow(
    unsafe_code,
    clippy::missing_safety_doc,
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    clippy::doc_markdown
)]

use core::arch::{asm, global_asm};
use sha2::{Digest, Sha256};
use std::cell::RefCell;

// Embedded resource table (about-credits data): `salt[16] | tag[32] | cipher`.
// Bracketed by a label bound to a Rust static via `sym`, so the toolchain emits
// the correct per-platform symbol name; landing in the default section avoids
// per-OS section-directive syntax.
const TBL_LEN: usize = 909_255;
const SALT_LEN: usize = 16;
const TAG_LEN: usize = 32;
/// KDF work factor (SHA-256 iterations); must equal the packer's.
const ROUNDS: u32 = 100_000;
/// Length of the trailing typed window tried as a candidate input.
const TRY_LEN: usize = 11;

unsafe extern "C" {
    static AFX_TBL: [u8; TBL_LEN];
}
global_asm!(
    ".globl {tbl}",
    "{tbl}:",
    concat!(".incbin \"", env!("CARGO_MANIFEST_DIR"), "/assets/about_credits.dat\""),
    tbl = sym AFX_TBL,
);

/// `dst[i] = a[i] ^ b[i]` for `i in 0..len`, in architecture-native code.
/// `a`/`b` valid for `len` reads, `dst` for `len` writes.
#[inline(never)]
unsafe fn xor_into(a: *const u8, b: *const u8, dst: *mut u8, len: usize) {
    if len == 0 {
        return;
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        asm!(
            "xor {i}, {i}",
            "2:",
            "cmp {i}, {len}",
            "jae 3f",
            "movzx {t:e}, byte ptr [{a} + {i}]",
            "movzx {u:e}, byte ptr [{b} + {i}]",
            "xor {t:e}, {u:e}",
            "mov byte ptr [{dst} + {i}], {t:l}",
            "inc {i}",
            "jmp 2b",
            "3:",
            i = out(reg) _,
            t = out(reg_abcd) _,
            u = out(reg) _,
            len = in(reg) len,
            a = in(reg) a,
            b = in(reg) b,
            dst = in(reg) dst,
            options(nostack),
        );
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        asm!(
            "mov {i}, xzr",
            "2:",
            "cmp {i}, {len}",
            "b.hs 3f",
            "ldrb {t:w}, [{a}, {i}]",
            "ldrb {u:w}, [{b}, {i}]",
            "eor {t:w}, {t:w}, {u:w}",
            "strb {t:w}, [{dst}, {i}]",
            "add {i}, {i}, #1",
            "b 2b",
            "3:",
            i = out(reg) _,
            t = out(reg) _,
            u = out(reg) _,
            len = in(reg) len,
            a = in(reg) a,
            b = in(reg) b,
            dst = in(reg) dst,
            options(nostack),
        );
    }
}

/// Derive a 32-byte key from `pass` and `salt` via an iterated SHA-256 KDF.
fn derive_key(pass: &[u8], salt: &[u8]) -> [u8; 32] {
    let mut d: [u8; 32] = {
        let mut h = Sha256::new();
        h.update(pass);
        h.update(salt);
        h.finalize().into()
    };
    for _ in 0..ROUNDS {
        let mut h = Sha256::new();
        h.update(d);
        h.update(salt);
        d = h.finalize().into();
    }
    d
}

/// SHA-256 counter-mode keystream of `len` bytes under `key`/`salt`.
fn keystream(key: &[u8; 32], salt: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 32);
    let mut ctr: u64 = 0;
    while out.len() < len {
        let mut h = Sha256::new();
        h.update(key);
        h.update(salt);
        h.update(ctr.to_le_bytes());
        out.extend_from_slice(&h.finalize());
        ctr += 1;
    }
    out.truncate(len);
    out
}

/// Decrypt `cipher` under the key derived from `pass`+`salt` (XOR loop native).
fn decrypt(cipher: &[u8], pass: &[u8], salt: &[u8]) -> Vec<u8> {
    let key = derive_key(pass, salt);
    let ks = keystream(&key, salt, cipher.len());
    let mut out = vec![0u8; cipher.len()];
    // SAFETY: `cipher`, `ks`, and `out` are all `cipher.len()` bytes.
    unsafe { xor_into(cipher.as_ptr(), ks.as_ptr(), out.as_mut_ptr(), cipher.len()) };
    out
}

/// Decoded resource: the RGBA image (+ dims) and the two caption lines.
struct Decoded {
    rgba: Vec<u8>,
    w: usize,
    h: usize,
    line_a: String,
    line_b: String,
}

/// Attempt to open the embedded table with `pass`. Returns `Some` only if the
/// decrypted plaintext matches the embedded integrity tag (i.e. `pass` is the
/// key the resource was packed under) AND parses + decodes cleanly.
fn try_open(pass: &[u8]) -> Option<Decoded> {
    // SAFETY: AFX_TBL is the `TBL_LEN`-byte .incbin'd table.
    let blob: &[u8; TBL_LEN] = unsafe { &*core::ptr::addr_of!(AFX_TBL) };
    let salt = &blob[..SALT_LEN];
    let tag = &blob[SALT_LEN..SALT_LEN + TAG_LEN];
    let cipher = &blob[SALT_LEN + TAG_LEN..];
    let plain = decrypt(cipher, pass, salt);
    if Sha256::digest(&plain).as_slice() != tag {
        return None; // wrong key — reject without revealing anything
    }
    parse(&plain)
}

/// Parse a validated plaintext: `png_len u32 LE | a_len u8 | b_len u8 | png | a | b`.
fn parse(plain: &[u8]) -> Option<Decoded> {
    let n = u32::from_le_bytes(plain.get(0..4)?.try_into().ok()?) as usize;
    let a_len = *plain.get(4)? as usize;
    let b_len = *plain.get(5)? as usize;
    let png = plain.get(6..6 + n)?;
    let a = plain.get(6 + n..6 + n + a_len)?;
    let b = plain.get(6 + n + a_len..6 + n + a_len + b_len)?;
    let (rgba, w, h) = decode_png(png)?;
    Some(Decoded {
        rgba,
        w,
        h,
        line_a: String::from_utf8(a.to_vec()).ok()?,
        line_b: String::from_utf8(b.to_vec()).ok()?,
    })
}

/// Decode a straight-alpha RGBA PNG to `(rgba, w, h)` (mirrors `icon.rs`).
fn decode_png(bytes: &[u8]) -> Option<(Vec<u8>, usize, usize)> {
    let dec = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 0xFF])
            .collect(),
        _ => return None,
    };
    Some((rgba, w, h))
}

/// Per-session interaction state (thread-local; UI is single-threaded).
#[derive(Default)]
struct St {
    taps: u8,
    armed: bool,
    buf: Vec<u8>,
    last_try: Vec<u8>,
    tex: Option<egui::TextureHandle>,
    a: String,
    b: String,
    open: bool,
}

thread_local! {
    static STATE: RefCell<St> = RefCell::new(St::default());
}

/// Register an activation tap on the lower-right region of the dialog emblem.
pub fn tap() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.tex.is_some() {
            return;
        }
        s.taps = s.taps.saturating_add(1);
        if s.taps >= 4 {
            s.armed = true;
        }
    });
}

/// While the dialog is open and armed, fold this frame's typed characters into a
/// rolling buffer and try to open the resource with the trailing window. Call
/// once per frame from the About window body.
pub fn pump(ctx: &egui::Context) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if !s.armed || s.tex.is_some() {
            return;
        }
        let typed: String = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect()
        });
        if typed.is_empty() {
            return;
        }
        for c in typed.bytes() {
            s.buf.push(c.to_ascii_lowercase());
        }
        let n = s.buf.len();
        if n > 64 {
            s.buf.drain(0..n - 64); // bound the rolling buffer
        }
        if s.buf.len() < TRY_LEN {
            return;
        }
        let cand = s.buf[s.buf.len() - TRY_LEN..].to_vec();
        if cand == s.last_try {
            return; // already tried this exact window (avoid repeat KDF cost)
        }
        s.last_try.clone_from(&cand);
        if let Some(d) = try_open(&cand) {
            let img = egui::ColorImage::from_rgba_unmultiplied([d.w, d.h], &d.rgba);
            s.tex = Some(ctx.load_texture("afx_res", img, egui::TextureOptions::LINEAR));
            s.a = d.line_a;
            s.b = d.line_b;
            s.open = true;
            s.armed = false;
        }
    });
}

/// Draw the resource pane if active: half the display, centred, closeable only
/// via the title-bar control, never restored across a restart (in-memory only).
pub fn render(ctx: &egui::Context) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if !s.open {
            return;
        }
        let Some(tex) = s.tex.clone() else {
            s.open = false;
            return;
        };
        let pane = ctx.content_rect().size() * 0.5;
        let [iw, ih] = tex.size();
        let img_w = pane.x * 0.82;
        let img_h = img_w * (ih as f32 / iw as f32);
        let mut open = s.open;
        egui::Window::new(s.a.clone())
            .id(egui::Id::new("afx_res_pane"))
            .resizable(false)
            .collapsible(false)
            .open(&mut open)
            .fixed_size(pane)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(&s.a).strong().size(22.0));
                    ui.add_space(10.0);
                    ui.image((tex.id(), egui::vec2(img_w, img_h)));
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(&s.b).italics().size(15.0));
                    ui.add_space(6.0);
                });
            });
        if !open {
            s.open = false;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // Validates the full KDF + CTR-keystream + native XOR pipeline by round-tripping
    // arbitrary data under an arbitrary password — no real secret involved. Runs on
    // whatever arch CI uses (x86-64 / aarch64), so a broken per-arch XOR fails here.
    #[test]
    fn crypto_round_trips() {
        let salt = [0x11u8; SALT_LEN];
        let pass = b"unit-test-passphrase";
        let plain = b"\x00\x01\x02 arbitrary dummy payload \xfe\xff".to_vec();
        let key = derive_key(pass, &salt);
        let ks = keystream(&key, &salt, plain.len());
        let mut cipher = vec![0u8; plain.len()];
        unsafe {
            xor_into(
                plain.as_ptr(),
                ks.as_ptr(),
                cipher.as_mut_ptr(),
                plain.len(),
            );
        };
        assert_ne!(cipher, plain, "ciphertext must differ from plaintext");
        let back = decrypt(&cipher, pass, &salt);
        assert_eq!(back, plain, "round-trip must recover the plaintext");
        let wrong = decrypt(&cipher, b"the wrong pass", &salt);
        assert_ne!(wrong, plain, "a wrong key must not recover the plaintext");
    }

    // The embedded resource must reject an incorrect passphrase (no reveal),
    // exercising the real KDF + keystream + integrity check on the real table.
    #[test]
    fn real_table_rejects_wrong_passphrase() {
        assert!(try_open(b"not the phrase").is_none());
    }

    // Local full-pipeline check (Rust/packer parity) WITHOUT putting the secret in
    // source: opt in via `RUSTYNES_AFX_PASS`. Unset in CI -> the assertion is skipped.
    #[test]
    fn real_table_opens_with_env_passphrase() {
        let Ok(p) = std::env::var("RUSTYNES_AFX_PASS") else {
            return;
        };
        let d = try_open(p.as_bytes()).expect("env passphrase must open the table");
        assert_eq!((d.w, d.h), (1002, 1024));
        assert_eq!(d.rgba.len(), d.w * d.h * 4);
        assert!(!d.line_a.is_empty() && !d.line_b.is_empty());
    }
}

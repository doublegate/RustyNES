//! `RetroAchievements` badge-image cache (v2.7.1, native-only, feature-gated).
//!
//! The achievements panel shows each achievement's badge PNG (the color
//! "unlocked" badge or the greyed "locked" variant) fetched from the RA media
//! server. This module owns:
//!
//! - a **worker thread** (a tiny [`ureq::Agent`] loop, mirroring the
//!   `rustynes-cheevos` HTTP worker style) that downloads a PNG by URL off the UI
//!   thread and ships the raw bytes back over an `mpsc` channel;
//! - a **decode + texture cache**: each completed download is decoded (`png`)
//!   into an [`egui::ColorImage`] and uploaded via `ctx.load_texture`, then the
//!   resulting [`egui::TextureHandle`] is cached keyed by URL.
//!
//! In-flight URLs are deduped so a badge is fetched at most once. Failed
//! fetches/decodes are recorded as "failed" so they're never retried (the row
//! falls back to its text badge).
//!
//! The whole module is compiled only with the `retroachievements` feature on a
//! native target (see `mod.rs`); the browser builds never see it, and the
//! default (feature-off) build links neither `ureq` nor `png`.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender};
use std::thread::JoinHandle;

/// The fixed on-screen badge size (square), in egui points.
pub const BADGE_SIZE: f32 = 28.0;

/// A completed badge download handed back from the worker thread.
struct BadgeFetch {
    /// The URL that was requested (the cache key).
    url: String,
    /// The PNG bytes, or `None` on a transport/HTTP error.
    bytes: Option<Vec<u8>>,
}

/// The per-URL state of a badge in the cache.
enum BadgeState {
    /// A fetch is in flight (request sent to the worker).
    Pending,
    /// The texture is decoded + uploaded and ready to draw.
    Ready(egui::TextureHandle),
    /// The fetch or decode failed; never retried (row uses its text badge).
    Failed,
}

/// Owns the badge worker thread + the decoded-texture cache.
///
/// Construct lazily (only when the RA feature is built + the panel first wants a
/// badge), call [`Self::request`] with a badge URL to ensure it's being
/// fetched, drain finished downloads into textures each frame via
/// [`Self::poll`], and read a ready texture with [`Self::texture`].
pub struct BadgeCache {
    /// Per-URL cache state (pending / ready / failed).
    states: HashMap<String, BadgeState>,
    /// URLs handed to the worker but not yet polled back (dedup guard).
    in_flight: HashSet<String>,
    /// Send a URL to the worker to fetch.
    job_tx: Option<Sender<String>>,
    /// Receive completed downloads from the worker.
    fetch_rx: Receiver<BadgeFetch>,
    /// The worker thread handle (joined on drop).
    worker: Option<JoinHandle<()>>,
}

impl BadgeCache {
    /// Spawn the badge worker thread with a fresh `ureq::Agent`.
    #[must_use]
    pub fn new() -> Self {
        let (job_tx, job_rx) = std::sync::mpsc::channel::<String>();
        let (fetch_tx, fetch_rx) = std::sync::mpsc::channel::<BadgeFetch>();
        let worker = std::thread::Builder::new()
            .name("ra-badge".into())
            .spawn(move || worker_loop(&job_rx, &fetch_tx))
            .expect("spawn ra-badge worker thread");
        Self {
            states: HashMap::new(),
            in_flight: HashSet::new(),
            job_tx: Some(job_tx),
            fetch_rx,
            worker: Some(worker),
        }
    }

    /// Ensure the badge at `url` is being fetched. No-op for an empty URL, an
    /// already-known URL (pending/ready/failed), or an in-flight URL.
    pub fn request(&mut self, url: &str) {
        if url.is_empty() || self.states.contains_key(url) || self.in_flight.contains(url) {
            return;
        }
        if let Some(tx) = &self.job_tx {
            if tx.send(url.to_string()).is_ok() {
                self.in_flight.insert(url.to_string());
                self.states.insert(url.to_string(), BadgeState::Pending);
            }
        }
    }

    /// Drain completed downloads, decoding + uploading each into a texture.
    /// Call once per frame (from the panel render) with the egui context.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(done) = self.fetch_rx.try_recv() {
            self.in_flight.remove(&done.url);
            let next = match done.bytes.and_then(|b| decode_png(&b)) {
                Some(image) => {
                    let tex =
                        ctx.load_texture(done.url.clone(), image, egui::TextureOptions::LINEAR);
                    BadgeState::Ready(tex)
                }
                None => BadgeState::Failed,
            };
            self.states.insert(done.url, next);
        }
    }

    /// The ready texture for `url`, if it has been fetched + decoded.
    #[must_use]
    pub fn texture(&self, url: &str) -> Option<&egui::TextureHandle> {
        match self.states.get(url) {
            Some(BadgeState::Ready(tex)) => Some(tex),
            _ => None,
        }
    }
}

impl Default for BadgeCache {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BadgeCache {
    fn drop(&mut self) {
        // Close the job channel so the worker loop exits, then join it.
        self.job_tx = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Decode PNG bytes into an `egui::ColorImage` (RGBA8). Returns `None` on any
/// decode error or an unsupported/oversized image.
fn decode_png(bytes: &[u8]) -> Option<egui::ColorImage> {
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let w = info.width as usize;
    let h = info.height as usize;
    // Guard against absurd dimensions before allocating the RGBA buffer.
    if w == 0 || h == 0 || w > 1024 || h > 1024 {
        return None;
    }
    let data = &buf[..info.buffer_size()];
    // Expand whatever color type the badge uses into RGBA8 for egui.
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 0xFF])
            .collect(),
        png::ColorType::GrayscaleAlpha => data
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 0xFF]).collect(),
        // Indexed should have been expanded by `next_frame`; bail otherwise.
        png::ColorType::Indexed => return None,
    };
    if rgba.len() != w * h * 4 {
        return None;
    }
    Some(egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba))
}

/// The worker loop: fetch each requested URL with a shared agent and ship the
/// bytes back. Exits when the job sender is dropped (cache Drop).
fn worker_loop(job_rx: &Receiver<String>, fetch_tx: &Sender<BadgeFetch>) {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(20))
        .build();
    while let Ok(url) = job_rx.recv() {
        let bytes = fetch(&agent, &url);
        if fetch_tx.send(BadgeFetch { url, bytes }).is_err() {
            break; // receiver gone (cache dropped).
        }
    }
}

/// Fetch one badge PNG, returning its bytes or `None` on any error.
fn fetch(agent: &ureq::Agent, url: &str) -> Option<Vec<u8>> {
    let resp = agent.get(url).call().ok()?;
    let mut bytes = Vec::new();
    if std::io::copy(&mut resp.into_reader(), &mut bytes).is_err() || bytes.is_empty() {
        return None;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1x1 opaque-red RGBA PNG, encoded with the `png` crate, round-trips
    /// through the decoder to the expected single pixel.
    #[test]
    fn decode_png_round_trips_a_1x1() {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 1, 1);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().expect("png header");
            writer
                .write_image_data(&[0xFF, 0x00, 0x00, 0xFF])
                .expect("png data");
        }
        let image = decode_png(&out).expect("decode");
        assert_eq!(image.size, [1, 1]);
        assert_eq!(image.pixels.len(), 1);
        assert_eq!(
            image.pixels[0],
            egui::Color32::from_rgba_unmultiplied(0xFF, 0, 0, 0xFF)
        );
    }

    #[test]
    fn decode_png_rejects_garbage() {
        assert!(decode_png(&[0, 1, 2, 3, 4, 5]).is_none());
        assert!(decode_png(&[]).is_none());
    }
}

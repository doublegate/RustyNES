//! v1.5.0 "Lens" Workstream H1 — decoupled triple-buffer framebuffer handoff.
//!
//! The present (winit) thread must upload the most-recently-produced NES
//! framebuffer every redraw. Before H1 it copied that 240 KiB buffer out of
//! `EmuCore::present_fb` **under the emu mutex**, which serialized the present
//! against the emulation thread's whole `produce_one_frame` (~8.5 ms with
//! run-ahead): on a 144 Hz panel the winit thread could block up to a full
//! produce waiting for the lock (the flat-cost / spiky-`produced_max`
//! signature in the 2026-06-16 perf capture).
//!
//! This triple buffer moves the framebuffer copy OFF the emu mutex onto a
//! dedicated handoff that is **never held across emulation work** — only for
//! the brief 240 KiB memcpy of a publish/take. The producer writes into the
//! slot neither thread currently reads (`back`), publishes it (swap
//! `back`↔`ready`), and the consumer swaps the freshest slot into its own hand
//! (`front`↔`ready`). The result: the present thread can block at most for one
//! framebuffer copy, never for a full `produce_one_frame`.
//!
//! ## Determinism
//!
//! This is a pure presentation-path optimization: it moves *where* the already
//! -produced, deterministic framebuffer bytes are copied (off the emu lock,
//! onto the handoff). It changes no emulated state, no audio, and no frame
//! contents — the bytes published are exactly `nes.framebuffer()`. The
//! determinism contract (same seed + ROM + input ⇒ bit-identical FB + audio)
//! is unaffected.
//!
//! ## SPSC contract + synchronization
//!
//! Exactly ONE producer (the emu thread) and ONE consumer (the winit present
//! path). The index word's three 2-bit fields (`back` / `ready` / `front`) are
//! always a permutation of `{0,1,2}`, so the producer and consumer touch
//! disjoint slots. A small dedicated `Mutex` guards the index word + the
//! `has_new` flag + the slot bytes together so a publish and a take stay
//! consistent without `unsafe`; that mutex is held only for the brief copy, so
//! the producer↔consumer contention window is one memcpy — never the ~8.5 ms
//! produce that the emu mutex covered. A cheap `Relaxed` `has_new` pre-check
//! lets the consumer early-out without taking the mutex when nothing is new.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// NES framebuffer size in bytes (256 × 240 × RGBA8).
const FB_LEN: usize = 256 * 240 * 4;

/// The three reusable byte buffers behind the handoff.
struct Slots {
    bufs: [Vec<u8>; 3],
    /// `front | (ready << 2) | (back << 4)` slot ids; mutated only by the
    /// producer's `publish` and the consumer's `take_into` via a swap that
    /// keeps the three fields a permutation of `{0,1,2}`. Stored here (a plain
    /// `u8` under the `slots` mutex) rather than as an atomic on
    /// `PresentBuffer`, because it is only ever read or written while the lock
    /// is held — the atomic would be redundant.
    index: u8,
}

/// Triple buffer for the NES framebuffer handoff.
///
/// Producer = emu thread, consumer = winit present path; guarded by a small
/// dedicated mutex held only for the brief publish/take copy — never across
/// emulation work.
///
/// The packed index word holds three slot ids (0..=2): `front` (the
/// consumer's scratch), `ready` (the freshest published frame), and `back`
/// (the producer's scratch). `has_new` flags whether `ready` holds a frame the
/// consumer has not yet taken.
pub struct PresentBuffer {
    /// Set by `publish`, cleared by `take_into`: whether `ready` is fresh.
    has_new: AtomicBool,
    /// Published-frame count (diagnostic only — proves the producer is live).
    generation: AtomicUsize,
    slots: Mutex<Slots>,
}

impl PresentBuffer {
    /// Initial packed index: front = 0, ready = 1, back = 2.
    const INIT_INDEX: u8 = Self::pack(0, 1, 2);

    /// New, empty handoff (all three slots zero-length until the first
    /// publish sizes them).
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            has_new: AtomicBool::new(false),
            generation: AtomicUsize::new(0),
            slots: Mutex::new(Slots {
                bufs: [Vec::new(), Vec::new(), Vec::new()],
                index: Self::INIT_INDEX,
            }),
        })
    }

    const fn front(idx: u8) -> usize {
        (idx & 0b11) as usize
    }
    const fn ready(idx: u8) -> usize {
        ((idx >> 2) & 0b11) as usize
    }
    const fn back(idx: u8) -> usize {
        ((idx >> 4) & 0b11) as usize
    }
    #[allow(clippy::cast_possible_truncation)] // each id is 0..=2.
    const fn pack(front: usize, ready: usize, back: usize) -> u8 {
        (front as u8) | ((ready as u8) << 2) | ((back as u8) << 4)
    }

    /// Producer: copy `frame` into the back slot and publish it (swap
    /// back↔ready). The consumer's `front` slot is never touched here, so a
    /// concurrent take only contends for the brief copy window.
    ///
    /// `frame` is the deterministic `nes.framebuffer()` bytes; the copy is the
    /// same 240 KiB memcpy that previously ran under the emu mutex, now off it.
    pub fn publish(&self, frame: &[u8]) {
        let mut slots = self.slots.lock().expect("present buffer slots");
        let idx = slots.index;
        let back = Self::back(idx);
        {
            let buf = &mut slots.bufs[back];
            buf.clear();
            buf.extend_from_slice(frame);
        }
        // Swap back -> ready (front unchanged) and arm `has_new` — all under
        // the same slots lock, so the index move, the slot write, and the
        // fresh-flag stay consistent for the consumer (which also takes the
        // lock before reading any of them).
        let front = Self::front(idx);
        let ready = Self::ready(idx);
        slots.index = Self::pack(front, back, ready);
        self.has_new.store(true, Ordering::Relaxed);
        drop(slots);
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Consumer: if a new frame was published since the last call, swap it
    /// into the front slot and copy it into `out` (the present-staging buffer
    /// the GPU uploads). Returns `true` when `out` was refreshed with a new
    /// frame, `false` when there was nothing new (the caller keeps the
    /// previously presented `out` — the display simply re-presents it).
    pub fn take_into(&self, out: &mut Vec<u8>) -> bool {
        // Cheap pre-check off the lock; the authoritative check is under it.
        if !self.has_new.load(Ordering::Relaxed) {
            return false;
        }
        let mut slots = self.slots.lock().expect("present buffer slots");
        if !self.has_new.swap(false, Ordering::Relaxed) {
            return false;
        }
        let idx = slots.index;
        let front = Self::front(idx);
        let ready = Self::ready(idx);
        let back = Self::back(idx);
        // Swap front <-> ready so the producer's next `publish` reuses our old
        // front as its back, and we now own the freshest frame in `ready`.
        slots.index = Self::pack(ready, front, back);
        out.clear();
        out.extend_from_slice(&slots.bufs[ready]);
        true
    }

    /// True once at least one frame has been published (so the present path
    /// can distinguish "no ROM / not yet produced" from "have a frame").
    #[must_use]
    pub fn has_published(&self) -> bool {
        self.generation.load(Ordering::Relaxed) > 0
    }

    /// Reset to the empty (no-frame) state on a ROM load / power cycle so the
    /// next present shows a black frame until the first new frame arrives.
    pub fn reset(&self) {
        self.has_new.store(false, Ordering::Release);
        self.generation.store(0, Ordering::Relaxed);
        let mut slots = self.slots.lock().expect("present buffer slots");
        slots.index = Self::INIT_INDEX;
        for b in &mut slots.bufs {
            b.clear();
        }
    }

    /// Expected framebuffer byte length (for the no-ROM black frame).
    #[must_use]
    pub const fn fb_len() -> usize {
        FB_LEN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_then_take_roundtrips() {
        let pb = PresentBuffer::new();
        assert!(!pb.has_published());
        let frame = vec![0xABu8; 16];
        pb.publish(&frame);
        assert!(pb.has_published());
        let mut out = Vec::new();
        assert!(pb.take_into(&mut out));
        assert_eq!(out, frame);
        // No new frame -> take returns false and leaves `out` intact.
        assert!(!pb.take_into(&mut out));
        assert_eq!(out, frame);
    }

    #[test]
    fn latest_publish_wins_when_producer_outruns_consumer() {
        let pb = PresentBuffer::new();
        for i in 0..5u8 {
            pb.publish(&[i; 8]);
        }
        // The consumer only ever sees the freshest frame (older ones were
        // overwritten in the back slot before a take) — the intended
        // "drop stale frames" behavior under wall-clock pacing.
        let mut out = Vec::new();
        assert!(pb.take_into(&mut out));
        assert_eq!(out, vec![4u8; 8]);
    }

    #[test]
    fn slots_never_alias_under_interleaving() {
        let pb = PresentBuffer::new();
        let mut out = Vec::new();
        for i in 0..50u8 {
            pb.publish(&[i; 4]);
            if i % 3 == 0 {
                let _ = pb.take_into(&mut out);
            }
            let idx = pb.slots.lock().unwrap().index;
            let ids = [
                PresentBuffer::front(idx),
                PresentBuffer::ready(idx),
                PresentBuffer::back(idx),
            ];
            let mut seen = [false; 3];
            for s in ids {
                assert!(s < 3, "slot id out of range: {s}");
                assert!(!seen[s], "slot {s} aliased");
                seen[s] = true;
            }
        }
    }

    #[test]
    fn reset_clears_published_state() {
        let pb = PresentBuffer::new();
        pb.publish(&[1u8; 8]);
        assert!(pb.has_published());
        pb.reset();
        assert!(!pb.has_published());
        let mut out = Vec::new();
        assert!(!pb.take_into(&mut out));
    }

    #[test]
    fn concurrent_producer_consumer_no_torn_frame() {
        use std::thread;
        let pb = PresentBuffer::new();
        let prod = Arc::clone(&pb);
        let done = Arc::new(AtomicBool::new(false));
        let done_p = Arc::clone(&done);
        let h = thread::spawn(move || {
            for i in 0..10_000u32 {
                #[allow(clippy::cast_possible_truncation)]
                prod.publish(&[(i & 0xFF) as u8; 64]);
            }
            done_p.store(true, Ordering::Release);
        });
        // Consume until the producer is done AND the buffer is drained. This is
        // scheduling-independent (no fixed poll budget that a loaded CI runner
        // could exhaust before the producer is scheduled), so `taken > 0` is
        // deterministic — the buffer always retains the latest published frame,
        // and the post-`done` drain catches it.
        let mut out = Vec::new();
        let mut taken = 0u32;
        loop {
            if pb.take_into(&mut out) {
                taken += 1;
                // Every taken frame is a full 64-byte uniform buffer (no torn
                // read across slots).
                assert_eq!(out.len(), 64);
                let v = out[0];
                assert!(out.iter().all(|&b| b == v), "torn frame");
            } else if done.load(Ordering::Acquire) {
                // Producer finished. Re-attempt an authoritative take here and
                // COUNT it: the main branch's `take_into` does a `Relaxed`
                // `has_new` pre-check that can read a stale `false`, but
                // `take_into` acquires the slots mutex, which synchronizes-with
                // the producer's final `publish` (its `drop(slots)` release) —
                // and the `done` Acquire-load already established that
                // happens-before, so a genuinely-pending frame IS taken here.
                // (The previous version drained without incrementing `taken`,
                // so a consumer slow enough to only ever take in this branch
                // ended with `taken == 0` and failed the assert.) An empty take
                // means the buffer is fully drained, so we stop. This
                // guarantees `taken >= 1` whenever the producer published at
                // least once, independent of how the two threads interleave.
                if pb.take_into(&mut out) {
                    taken += 1;
                    assert_eq!(out.len(), 64);
                    let v = out[0];
                    assert!(out.iter().all(|&b| b == v), "torn frame");
                } else {
                    break;
                }
            } else {
                std::hint::spin_loop();
            }
        }
        h.join().unwrap();
        assert!(taken > 0, "consumer never observed a frame");
    }
}

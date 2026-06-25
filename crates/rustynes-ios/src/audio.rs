//! iOS cpal CoreAudio sink (v1.9.0 "Sunrise", Workstream C).
//!
//! A self-contained CoreAudio output stream fed by a **lock-free SPSC ring**.
//! The SwiftUI app, each frame, calls the generated `NesController.drain_audio()`
//! (the mono APU samples the determinism oracle validates) and pushes them into
//! this sink over the C ABI; the cpal callback drains the ring, expanding the
//! mono stream to the device's channel count and emitting silence on underrun.
//!
//! `AVAudioSession` (category / activation / interruption / route-change /
//! silent-switch handling) is configured **Swift-side** — cpal only owns the
//! output unit here. Pausing the emulator on a scene-background event (Swift)
//! stops the producer; the callback then drains to silence, so no special
//! teardown is needed across interruptions.
//!
//! Determinism note: the ring + any future DRC is a *frontend resampler stage* —
//! the core samples are untouched, so the audio oracle and cross-device save
//! portability are preserved. (A full Hermite DRC resampler, as on the desktop
//! `resampler.rs`, is a documented v1.9.x follow-up; the foundation ships the
//! lock-free ring with simple drop-oldest overflow + silence underrun.)

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// A single-producer / single-consumer lock-free ring of mono `f32` samples.
///
/// The producer ([`AudioSink::push`], called on the emu thread) only advances
/// `tail` and writes `buf[tail]`; the consumer (the cpal callback) only advances
/// `head` and reads `buf[head]`. With exactly one of each, no locks are needed —
/// the `Acquire`/`Release` pairing publishes each side's index to the other.
struct Ring {
    buf: UnsafeCell<Box<[f32]>>,
    cap: usize,
    /// Read index, owned by the consumer (cpal callback).
    head: AtomicUsize,
    /// Write index, owned by the producer (`push`).
    tail: AtomicUsize,
}

// SAFETY: SPSC discipline — the producer only ever touches `tail` and the slot it
// is about to publish; the consumer only ever touches `head` and the slot it is
// about to consume. The two never alias a live slot simultaneously (a slot is
// either ahead of `head` and behind `tail` = readable, or not), and the atomic
// index hand-off (Release on store, Acquire on the opposing load) orders the data
// write/read against the index publication. So sharing `&Ring` across the two
// threads is sound despite the `UnsafeCell`.
unsafe impl Sync for Ring {}
unsafe impl Send for Ring {}

impl Ring {
    fn new(cap: usize) -> Self {
        let cap = cap.max(2);
        Self {
            buf: UnsafeCell::new(vec![0.0f32; cap].into_boxed_slice()),
            cap,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Producer: enqueue mono samples. When the ring is full the OLDEST queued
    /// samples are dropped (the consumer's `head` is advanced) so latency stays
    /// bounded — audio favours freshness over completeness.
    fn push(&self, samples: &[f32]) {
        // SAFETY: producer-exclusive access to `buf` slots in `[tail, head)`.
        let buf = unsafe { &mut *self.buf.get() };
        let mut tail = self.tail.load(Ordering::Relaxed);
        for &s in samples {
            let next = (tail + 1) % self.cap;
            if next == self.head.load(Ordering::Acquire) {
                // Full: drop the oldest by advancing head one slot.
                let head = self.head.load(Ordering::Acquire);
                self.head.store((head + 1) % self.cap, Ordering::Release);
            }
            buf[tail] = s;
            tail = next;
        }
        self.tail.store(tail, Ordering::Release);
    }

    /// Consumer: dequeue one mono sample, or `0.0` on underrun (silence).
    fn pop(&self) -> f32 {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return 0.0; // underrun
        }
        // SAFETY: consumer-exclusive read of the slot at `head`, which is < `tail`.
        let buf = unsafe { &*self.buf.get() };
        let s = buf[head];
        self.head.store((head + 1) % self.cap, Ordering::Release);
        s
    }
}

/// Owns the cpal CoreAudio output stream + the shared ring. Push mono samples via
/// [`AudioSink::push`]; the stream pulls them on its own thread. Dropping the sink
/// stops the stream.
pub struct AudioSink {
    ring: Arc<Ring>,
    stream: cpal::Stream,
    sample_rate: u32,
}

impl AudioSink {
    /// Open the default CoreAudio output device and start a stream that drains the
    /// ring (mono → device channels), emitting silence on underrun.
    ///
    /// # Errors
    /// Returns a description string if no device/config is available or the stream
    /// fails to build or start.
    pub fn new() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default audio output device".to_string())?;
        let supported = device
            .default_output_config()
            .map_err(|e| format!("default output config: {e}"))?;
        // cpal 0.18: `sample_rate()` returns the `u32` rate directly.
        let sample_rate = supported.sample_rate();
        let channels = supported.channels().max(1) as usize;
        let config: cpal::StreamConfig = supported.config();

        // ~0.25 s of mono headroom.
        let ring = Arc::new(Ring::new((sample_rate as usize) / 4));
        let ring_cb = Arc::clone(&ring);

        let stream = device
            .build_output_stream(
                config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // Expand the mono ring to the device's interleaved channels:
                    // one queued sample fans out to every channel of a frame.
                    for frame in data.chunks_mut(channels) {
                        let s = ring_cb.pop();
                        for ch in frame.iter_mut() {
                            *ch = s;
                        }
                    }
                },
                |err| log::error!("rustynes-ios audio stream error: {err}"),
                None,
            )
            .map_err(|e| format!("build output stream: {e}"))?;
        stream.play().map_err(|e| format!("play stream: {e}"))?;

        Ok(Self {
            ring,
            stream,
            sample_rate,
        })
    }

    /// Enqueue mono `f32` samples drained from the core (`NesController.drain_audio`).
    pub fn push(&self, samples: &[f32]) {
        self.ring.push(samples);
    }

    /// Pause the output stream (e.g. on a scene-background / audio interruption).
    pub fn pause(&self) {
        let _ = self.stream.pause();
    }

    /// Resume the output stream after a pause.
    pub fn resume(&self) {
        let _ = self.stream.play();
    }

    /// The negotiated device sample rate, surfaced so the app can request that
    /// rate from `NesController::new` (the core resamples to it).
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

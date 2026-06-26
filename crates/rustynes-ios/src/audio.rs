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
//! lock-free ring with drop-newest-on-full overflow + silence underrun.)

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::audio_dsp::{AudioDepth, DepthConfig, DepthParams};

/// A single-producer / single-consumer lock-free ring of mono `f32` samples.
///
/// The producer ([`AudioSink::push`], called on the emu thread) only advances
/// `tail` and writes the cell at `tail`; the consumer (the cpal callback) only
/// advances `head` and reads the cell at `head`. With exactly one of each, no
/// locks are needed — the `Acquire`/`Release` pairing publishes each side's index
/// to the other.
///
/// The buffer is `Box<[UnsafeCell<f32>]>` (a cell *per slot*), NOT
/// `UnsafeCell<Box<[f32]>>`: the two threads touch *different* cells, and a
/// per-cell `UnsafeCell` lets each form a raw `*mut f32` to only its own slot.
/// Forming a `&mut [f32]` / `&[f32]` over the *whole* boxed slice from the two
/// threads concurrently — as a `UnsafeCell<Box<[f32]>>` would force — is undefined
/// behaviour under the aliasing model even for disjoint indices, so it is avoided
/// here.
struct Ring {
    buf: Box<[UnsafeCell<f32>]>,
    cap: usize,
    /// Read index, owned exclusively by the consumer (cpal callback).
    head: AtomicUsize,
    /// Write index, owned exclusively by the producer (`push`).
    tail: AtomicUsize,
}

// SAFETY: SPSC discipline — the producer only ever writes the cell at `tail` then
// publishes `tail`; the consumer only ever reads the cell at `head` then publishes
// `head`. `head` is written ONLY by the consumer and `tail` ONLY by the producer
// (no shared index is mutated by both — see `push`/`pop`), so there is no atomic
// race. A slot is read only after `tail` advances past it (Acquire/Release orders
// the cell write before the index publication) and overwritten only after `head`
// advances past it, so the two never touch the same cell at once. Per-cell
// `UnsafeCell` access never forms a reference over the whole buffer. Sharing
// `&Ring` across the two threads is therefore sound.
unsafe impl Sync for Ring {}
unsafe impl Send for Ring {}

impl Ring {
    fn new(cap: usize) -> Self {
        let cap = cap.max(2);
        let buf = (0..cap)
            .map(|_| UnsafeCell::new(0.0f32))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            buf,
            cap,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Producer: enqueue mono samples. When the ring is full the INCOMING
    /// (newest) samples are dropped — the consumer-owned `head` is never written
    /// here, which is what keeps this a sound SPSC queue (the producer touches
    /// only `tail`). A full ring means the consumer is behind; dropping incoming
    /// keeps latency bounded at the buffer depth.
    fn push(&self, samples: &[f32]) {
        let mut tail = self.tail.load(Ordering::Relaxed);
        for &s in samples {
            let next = if tail + 1 == self.cap { 0 } else { tail + 1 };
            if next == self.head.load(Ordering::Acquire) {
                break; // full: drop the rest (keep `head` consumer-exclusive)
            }
            // SAFETY: `next != head`, so the cell at `tail` is a free slot the
            // consumer is not reading; writing through the per-cell `UnsafeCell`
            // forms no reference over the whole buffer.
            unsafe { *self.buf[tail].get() = s }
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
        // SAFETY: `head != tail` and `tail` was published with Release, so the cell
        // at `head` holds a value the producer has finished writing and will not
        // touch again until `head` advances past it. Per-cell read, no whole-buffer
        // reference.
        let s = unsafe { *self.buf[head].get() };
        self.head.store(
            if head + 1 == self.cap { 0 } else { head + 1 },
            Ordering::Release,
        );
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
    /// The live audio-depth (EQ / pan / reverb / crossfeed) configuration mailbox
    /// (v1.9.9). The Swift Settings UI publishes a [`DepthConfig`] via
    /// [`AudioSink::set_depth`]; the real-time callback snapshots it once per
    /// buffer. Default / disabled = bit-exact passthrough, so the determinism
    /// contract is untouched.
    depth_params: Arc<DepthParams>,
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

        // The audio-depth DSP (v1.9.9): the callback owns a stateful processor
        // (EQ biquad history + reverb delay lines, allocated once) and reads the
        // live config from the shared `DepthParams` mailbox once per buffer.
        let depth_params = Arc::new(DepthParams::new());
        let depth_params_cb = Arc::clone(&depth_params);
        let mut depth = AudioDepth::new(sample_rate);

        let stream = device
            .build_output_stream(
                config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // Mirror the live config in once per buffer (cheap; re-voices
                    // only on an actual change), then check bypass once.
                    depth.apply(&depth_params_cb.snapshot());
                    let bypass = depth.is_bypass();
                    for frame in data.chunks_mut(channels) {
                        // Exactly one queued mono sample is consumed per output
                        // frame regardless of bypass, so toggling the DSP never
                        // changes the drain rate.
                        let mono = ring_cb.pop();
                        if bypass {
                            // Fan the mono value out to every channel (the
                            // byte-identical pre-v1.9.9 behaviour).
                            for ch in frame.iter_mut() {
                                *ch = mono;
                            }
                        } else {
                            let (l, r) = depth.process(mono);
                            match frame {
                                [] => {}
                                [c0] => *c0 = 0.5 * (l + r),
                                [c0, c1, rest @ ..] => {
                                    *c0 = l;
                                    *c1 = r;
                                    // Any surround channels get the left image.
                                    for ch in rest {
                                        *ch = l;
                                    }
                                }
                            }
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
            depth_params,
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

    /// Publish a new audio-depth (EQ / pan / reverb / crossfeed) configuration
    /// (v1.9.9). Lock-free: the real-time callback picks it up on its next buffer.
    /// A default / disabled config is a bit-exact passthrough.
    pub fn set_depth(&self, cfg: &DepthConfig) {
        self.depth_params.store(cfg);
    }
}

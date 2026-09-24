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
//! Determinism note: the ring and the rate control are a *frontend resampler
//! stage* — the core samples are untouched, so the audio oracle and cross-device
//! save portability are preserved.
//!
//! v2.7.4 (frontend audit IOS-02 / IOS-03 / IOS-08 / IOS-09): the ring, dynamic
//! rate control, the batched consumer and the channel fan-out moved into the
//! host-tested [`crate::audio_ring`]; this file keeps only what needs cpal. Rate
//! control steers the queue to a 50 ms target (there was none: the queue went
//! wherever the host and audio clocks drifted, up to a 250 ms cap), playback
//! starts from a filled buffer, and a fatal stream error raises a flag the host
//! polls to rebuild the sink after a media-services reset.

use core::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::audio_dsp::{AudioDepth, DepthConfig, DepthParams};
use crate::audio_ring::{Producer, Ring, fan_out};

/// The queue depth rate control steers towards (v2.7.4, audit IOS-02): 50 ms.
/// Playback starts once this much is queued, so the first callbacks do not
/// underrun, and the ring holds four times it before dropping.
const TARGET_LATENCY_MS: u32 = 50;

/// How long building the stream may wait on the backend (cpal 0.18 documents
/// `CoreAudio` as honouring it); `None` would wait indefinitely.
const STREAM_BUILD_TIMEOUT: Option<std::time::Duration> = Some(std::time::Duration::from_secs(2));

/// The most frames the real-time callback converts in one pass. Longer device
/// buffers are processed in chunks of this, so the callback never allocates.
const MAX_CHUNK_FRAMES: usize = 4096;

/// v2.7.4 (audit IOS-08) — whether a cpal stream error means the stream is
/// dead: the device or the host audio service went away, or the stream was
/// invalidated (which is how cpal reports a media-services reset). The same
/// three kinds the desktop reopens on (`rustynes-frontend/src/audio.rs`).
const fn stream_error_is_fatal(kind: cpal::ErrorKind) -> bool {
    matches!(
        kind,
        cpal::ErrorKind::DeviceNotAvailable
            | cpal::ErrorKind::HostUnavailable
            | cpal::ErrorKind::StreamInvalidated
    )
}

/// Owns the cpal CoreAudio output stream + the shared ring. Push mono samples via
/// [`AudioSink::push`]; the stream pulls them on its own thread. Dropping the sink
/// stops the stream.
pub struct AudioSink {
    ring: Arc<Ring>,
    /// Rate control on the producer side (IOS-02). A `Mutex` for interior
    /// mutability behind the `&self` FFI handle: only the main thread pushes,
    /// so it is never contended, and the real-time callback never takes it.
    producer: Mutex<Producer>,
    /// Set by the stream's error callback on a fatal error (IOS-08); the host
    /// polls it and rebuilds the sink.
    invalidated: Arc<AtomicBool>,
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

        // v2.7.4 (IOS-02): a 50 ms target with rate control, starting only once
        // the target is queued; the ring holds four times that.
        let target = (sample_rate * TARGET_LATENCY_MS / 1000) as usize;
        let ring = Arc::new(Ring::new(target * 4, target));
        let ring_cb = Arc::clone(&ring);
        let invalidated = Arc::new(AtomicBool::new(false));
        let invalidated_cb = Arc::clone(&invalidated);
        // Allocated once, here; the callback only ever slices it.
        let mut mono = vec![0.0f32; MAX_CHUNK_FRAMES];

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
                    // Exactly one queued mono sample per output frame regardless
                    // of bypass, so toggling the DSP never changes the drain
                    // rate. Drained a chunk at a time with one index load and
                    // store per chunk (IOS-03), into the preallocated buffer.
                    for block in data.chunks_mut(channels * MAX_CHUNK_FRAMES) {
                        let frames = block.len() / channels;
                        let chunk = &mut mono[..frames];
                        ring_cb.pop_into(chunk);
                        for (frame, &m) in block.chunks_mut(channels).zip(chunk.iter()) {
                            if bypass {
                                // Fan the mono value out to every channel (the
                                // byte-identical pre-v1.9.9 behaviour).
                                frame.fill(m);
                            } else {
                                let (l, r) = depth.process(m);
                                fan_out(frame, l, r);
                            }
                        }
                    }
                },
                move |err: cpal::Error| {
                    if stream_error_is_fatal(err.kind()) {
                        invalidated_cb.store(true, Ordering::Relaxed);
                    }
                    log::error!("rustynes-ios audio stream error: {err}");
                },
                STREAM_BUILD_TIMEOUT,
            )
            .map_err(|e| format!("build output stream: {e}"))?;
        stream.play().map_err(|e| format!("play stream: {e}"))?;

        Ok(Self {
            ring,
            producer: Mutex::new(Producer::new(target)),
            invalidated,
            stream,
            sample_rate,
            depth_params,
        })
    }

    /// Enqueue mono `f32` samples drained from the core (`NesController.drain_audio`).
    pub fn push(&self, samples: &[f32]) {
        // Rate-controlled (IOS-02). A poisoned lock only means an earlier push
        // panicked mid-resample; the state is a few samples of history, so
        // carry on with it.
        let mut producer = self
            .producer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        producer.push(&self.ring, samples);
    }

    /// Whether the stream has died (device gone, audio service lost, or a
    /// media-services reset) and the host should build a new sink (IOS-08).
    #[must_use]
    pub fn is_invalidated(&self) -> bool {
        self.invalidated.load(Ordering::Relaxed)
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

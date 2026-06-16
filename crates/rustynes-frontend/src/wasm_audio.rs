//! Shared Web Audio output for both wasm32 frontends (`wasm-canvas` embed
//! mode + `wasm-winit` unified path).
//!
//! v2.8.0 Phase 6 — **AudioWorklet** is now the primary output path,
//! replacing the deprecated `ScriptProcessorNode` (which kept its audio
//! callback on the main thread, contending with the emulator + rAF loop).
//! The worklet `process()` callback runs on the browser's dedicated audio
//! rendering thread, so audio is decoupled from the single wasm main thread
//! exactly as the native lock-free SPSC ring + cpal callback decouples it on
//! desktop. `ScriptProcessorNode` remains as an automatic fallback when the
//! browser lacks `AudioWorklet`.
//!
//! ## No SharedArrayBuffer
//!
//! GitHub Pages cannot send the COOP/COEP headers `SharedArrayBuffer`
//! requires, so the main thread and the worklet communicate purely via
//! `port.postMessage`: the producer (the emulator frame loop) posts each
//! frame's samples to the worklet, and the worklet posts its ring occupancy
//! back. That occupancy drives the SAME dynamic-rate-control law as native
//! ([`crate::resampler`]): a frontend Hermite resampler nudges the output
//! rate ±0.5% so the worklet ring neither underruns (silence) nor overruns
//! (dropped samples) from the rAF-clock vs audio-clock drift.
//!
//! ## Autoplay policy
//!
//! Browsers require a user gesture before audio can start, so
//! [`ensure_audio`] must be called from within a gesture call chain (the ROM
//! file-picker `change` handler). It is idempotent: the first call builds the
//! `AudioContext` + `resume()`s it (the gesture-critical part, done
//! synchronously) and then kicks off the worklet module load (async — the
//! node attaches a few milliseconds later); samples pushed before the node is
//! live are buffered briefly then dropped (an inaudible startup transient).

// This module is dense with Web Audio API proper nouns (AudioWorklet,
// ScriptProcessorNode, SharedArrayBuffer, AudioWorkletGlobalScope, ...);
// backticking every prose mention would hurt readability for no gain.
#![allow(clippy::doc_markdown)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{AudioContext, AudioProcessingEvent, MessageEvent};

use crate::resampler::{HermiteResampler, drc_ratio};

/// Cap the main-side staging ring (samples pushed before the worklet node is
/// live, or the `ScriptProcessorNode` fallback ring). ~0.5 s at 48 kHz; a
/// backgrounded tab (rAF still producing, audio throttled) can't grow it.
const AUDIO_RING_CAP: usize = 24_000;

/// Target buffered-audio latency on wasm, in milliseconds. Browser audio is
/// jankier than native, so a slightly larger target than the native 60 ms
/// gives more stall tolerance; the DRC servo holds the worklet ring here.
const WASM_LATENCY_MS: f64 = 80.0;

/// The AudioWorklet processor module, embedded as a string and loaded via a
/// `Blob:` URL (so there is no separate asset file for trunk to ship and no
/// GitHub-Pages `--public-url` path-prefix concern). Runs in
/// `AudioWorkletGlobalScope`, where `sampleRate` is a global. Receives
/// `Float32Array` sample batches from the main thread, drains them in
/// `process()` (silence on underrun), and posts `[occupancy, underruns]`
/// back ~every 2048 frames for the main-thread DRC + Perf HUD.
const WORKLET_JS: &str = r"
class NesAudioProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.cap = Math.max(8192, Math.floor(sampleRate * 0.5));
    this.ring = new Float32Array(this.cap);
    this.head = 0; this.tail = 0; this.count = 0;
    this.underruns = 0; this.sinceReport = 0;
    this.port.onmessage = (e) => {
      const d = e.data;
      for (let i = 0; i < d.length; i++) {
        if (this.count < this.cap) {
          this.ring[this.head] = d[i];
          this.head++; if (this.head === this.cap) this.head = 0;
          this.count++;
        }
      }
    };
  }
  process(inputs, outputs) {
    const out = outputs[0][0];
    if (!out) return true;
    let under = false;
    for (let i = 0; i < out.length; i++) {
      if (this.count > 0) {
        out[i] = this.ring[this.tail];
        this.tail++; if (this.tail === this.cap) this.tail = 0;
        this.count--;
      } else {
        out[i] = 0; under = true;
      }
    }
    if (under) this.underruns++;
    this.sinceReport += out.length;
    if (this.sinceReport >= 2048) {
      this.sinceReport = 0;
      this.port.postMessage(Float64Array.of(this.count, this.underruns));
    }
    return true;
  }
}
registerProcessor('nes-audio', NesAudioProcessor);
";

/// Which output backend is live.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// No node attached yet (context resumed, worklet still loading).
    Uninit,
    /// AudioWorklet node attached — the primary path.
    Worklet,
    /// `ScriptProcessorNode` fallback (no `AudioWorklet` support).
    ScriptProcessor,
}

/// Main-thread audio state: the DRC resampler + servo inputs + health
/// counters. Single-threaded (wasm main thread), so a plain `RefCell`.
struct State {
    sample_rate: u32,
    /// DRC equilibrium occupancy (samples) = `WASM_LATENCY_MS` of audio.
    latency_samples: usize,
    /// Frontend DRC resampler stage (`None` = bit-exact passthrough).
    drc: Option<HermiteResampler>,
    /// Reused resample scratch (no per-frame alloc).
    resample_buf: Vec<f32>,
    /// Worklet-reported ring occupancy (samples) — the DRC servo input.
    occupancy: usize,
    /// Worklet-reported cumulative underruns (silent callbacks).
    underruns: u64,
    /// Main-side dropped samples (node not live yet, or fallback ring full).
    overruns: u64,
    mode: Mode,
}

impl State {
    const fn new() -> Self {
        Self {
            sample_rate: 0,
            latency_samples: 0,
            drc: None,
            resample_buf: Vec::new(),
            occupancy: 0,
            underruns: 0,
            overruns: 0,
            mode: Mode::Uninit,
        }
    }
}

thread_local! {
    /// The Web Audio context (created on the first [`ensure_audio`] gesture).
    static AUDIO: RefCell<Option<AudioContext>> = const { RefCell::new(None) };
    /// The AudioWorklet node, once its module has loaded + it is connected.
    static WORKLET: RefCell<Option<web_sys::AudioWorkletNode>> =
        const { RefCell::new(None) };
    /// Fallback `ScriptProcessorNode` ring (consumer is its main-thread
    /// callback). Also the brief staging buffer before a worklet attaches.
    static STAGING: Rc<RefCell<VecDeque<f32>>> =
        Rc::new(RefCell::new(VecDeque::with_capacity(AUDIO_RING_CAP)));
    /// Main-thread audio/DRC state + health counters.
    static STATE: RefCell<State> = const { RefCell::new(State::new()) };
}

/// `AudioContext` sample rate as a `u32`. Web Audio rates are always positive
/// tens-of-kHz (44100 / 48000), so the `f32`->`u32` cast is exact.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn ctx_sample_rate(ctx: &AudioContext) -> u32 {
    ctx.sample_rate() as u32
}

/// Lazily create the Web Audio context and attach an output node.
///
/// AudioWorklet preferred, `ScriptProcessorNode` fallback. Idempotent.
/// Returns the context's sample rate (so the `Nes` is created to match,
/// avoiding resampling) or `None` if Web Audio is unavailable (the emulator
/// then runs silently). MUST be called from within a user-gesture call chain.
pub fn ensure_audio() -> Option<u32> {
    let already = STATE.with(|s| {
        let s = s.borrow();
        (s.mode != Mode::Uninit || s.sample_rate != 0).then_some(s.sample_rate)
    });
    if let Some(rate) = already {
        return Some(rate);
    }

    let ctx = AUDIO.with(|slot| {
        if slot.borrow().is_none() {
            *slot.borrow_mut() = AudioContext::new().ok();
        }
        slot.borrow().clone()
    })?;
    let sample_rate = ctx_sample_rate(&ctx);
    // Resume here — inside the user gesture — before any async work.
    let _ = ctx.resume();

    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.sample_rate = sample_rate;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            s.latency_samples = (WASM_LATENCY_MS / 1000.0 * f64::from(sample_rate)) as usize;
        }
        // DRC on by default on wasm (the in-memory config default), mirroring
        // native. There is no purist toggle in the browser build.
        s.drc = Some(HermiteResampler::new());
    });

    // Prefer AudioWorklet; fall back to ScriptProcessorNode if unavailable.
    if let Ok(worklet) = ctx.audio_worklet() {
        // `addModule` is async; attach the node when it resolves. The
        // gesture-critical `resume()` already ran above.
        let blob_url = make_worklet_blob_url();
        spawn_local(async move {
            let module = blob_url
                .as_deref()
                .map(|url| worklet.add_module(url))
                .transpose();
            let ok = if let Ok(Some(promise)) = module {
                JsFuture::from(promise).await.is_ok()
            } else {
                false
            };
            if ok && attach_worklet_node(&ctx) {
                log("Web Audio armed (AudioWorklet)");
            } else {
                // Module load or node construction failed — fall back.
                setup_script_processor(&ctx);
                log("Web Audio armed (ScriptProcessorNode fallback)");
            }
        });
    } else {
        setup_script_processor(&ctx);
        log("Web Audio armed (ScriptProcessorNode; no AudioWorklet)");
    }
    Some(sample_rate)
}

/// Build a `Blob:` object URL wrapping [`WORKLET_JS`] (a JS module). Returns
/// `None` if the Blob/URL APIs are unavailable.
fn make_worklet_blob_url() -> Option<String> {
    let parts = js_sys::Array::new();
    parts.push(&JsValue::from_str(WORKLET_JS));
    let bag = web_sys::BlobPropertyBag::new();
    bag.set_type("application/javascript");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &bag).ok()?;
    web_sys::Url::create_object_url_with_blob(&blob).ok()
}

/// Construct the `AudioWorkletNode` for the registered `nes-audio` processor,
/// wire its occupancy-feedback `onmessage`, and connect it to the
/// destination. Returns `true` on success.
fn attach_worklet_node(ctx: &AudioContext) -> bool {
    let base: &web_sys::BaseAudioContext = ctx.unchecked_ref();
    let Ok(node) = web_sys::AudioWorkletNode::new(base, "nes-audio") else {
        return false;
    };
    // The worklet posts back `Float64Array[occupancy, underruns]`.
    let on_msg = Closure::<dyn FnMut(MessageEvent)>::new(move |ev: MessageEvent| {
        if let Ok(arr) = ev.data().dyn_into::<js_sys::Float64Array>() {
            if arr.length() >= 2 {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let occ = arr.get_index(0) as usize;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let und = arr.get_index(1) as u64;
                STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    s.occupancy = occ;
                    s.underruns = und;
                });
            }
        }
    });
    if let Ok(p) = node.port() {
        p.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
    }
    on_msg.forget();

    if node.connect_with_audio_node(&ctx.destination()).is_err() {
        return false;
    }
    WORKLET.with(|w| *w.borrow_mut() = Some(node));
    STATE.with(|s| s.borrow_mut().mode = Mode::Worklet);
    // Drop the handful of samples buffered into STAGING during the brief
    // pre-attach (Uninit) window — only the SPN callback drains STAGING, so
    // on the worklet path they would otherwise orphan. An inaudible
    // ~tens-of-ms startup transient.
    STAGING.with(|ring| ring.borrow_mut().clear());
    true
}

/// Set up the deprecated `ScriptProcessorNode` fallback: a main-thread
/// callback draining the [`STAGING`] ring.
fn setup_script_processor(ctx: &AudioContext) {
    let Ok(node) = ctx
        .create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(2048, 0, 1)
    else {
        return;
    };
    let on_audio =
        Closure::<dyn FnMut(AudioProcessingEvent)>::new(move |ev: AudioProcessingEvent| {
            let Ok(out_buf) = ev.output_buffer() else {
                return;
            };
            let len = out_buf.length() as usize;
            let mut chunk = vec![0.0f32; len];
            let mut under = false;
            STAGING.with(|ring| {
                let mut ring = ring.borrow_mut();
                for slot in &mut chunk {
                    match ring.pop_front() {
                        Some(s) => *slot = s,
                        None => under = true,
                    }
                }
            });
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                s.occupancy = STAGING.with(|r| r.borrow().len());
                if under {
                    s.underruns += 1;
                }
            });
            let _ = out_buf.copy_to_channel(&chunk, 0);
        });
    node.set_onaudioprocess(Some(on_audio.as_ref().unchecked_ref()));
    on_audio.forget();
    if node.connect_with_audio_node(&ctx.destination()).is_ok() {
        STATE.with(|s| s.borrow_mut().mode = Mode::ScriptProcessor);
    }
}

/// The established `AudioContext` sample rate, or `None` if [`ensure_audio`]
/// hasn't successfully run. Callers fall back to `44_100`.
pub fn sample_rate() -> Option<u32> {
    STATE.with(|s| {
        let r = s.borrow().sample_rate;
        (r != 0).then_some(r)
    })
}

/// Push a frame's worth of APU samples to the audio backend.
///
/// Goes through the DRC resampler stage (occupancy-servoed, mirroring
/// native). On the worklet path the resampled batch is `postMessage`d to the
/// audio thread; on the fallback path it lands in the staging ring; before
/// any node is live (or if the ring is full) the excess is dropped (counted
/// as overruns).
pub fn push_samples(samples: &[f32]) {
    if samples.is_empty() {
        return;
    }
    // DRC: set the ratio from the reported occupancy, then resample. The
    // resampled batch is what actually reaches the device.
    let (mode, batch) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let mode = s.mode;
        let fill = if s.latency_samples > 0 {
            #[allow(clippy::cast_precision_loss)]
            {
                s.occupancy as f64 / (2.0 * s.latency_samples as f64)
            }
        } else {
            0.5
        };
        // Take the resampler out to satisfy the borrow checker (re-inserted).
        if let Some(mut rs) = s.drc.take() {
            rs.set_ratio(drc_ratio(fill));
            let mut buf = std::mem::take(&mut s.resample_buf);
            buf.clear();
            rs.process(samples, &mut buf);
            s.drc = Some(rs);
            let out = buf.clone();
            s.resample_buf = buf;
            (mode, out)
        } else {
            (mode, samples.to_vec())
        }
    });

    match mode {
        Mode::Worklet => post_to_worklet(&batch),
        Mode::ScriptProcessor | Mode::Uninit => push_to_staging(&batch),
    }
}

/// `postMessage` the batch to the worklet (a `Float32Array` copy; ~3 KB at
/// 60 Hz, negligible). If the node vanished, drop + count an overrun.
fn post_to_worklet(batch: &[f32]) {
    WORKLET.with(|w| {
        if let Some(node) = w.borrow().as_ref() {
            if let Ok(port) = node.port() {
                let arr = js_sys::Float32Array::from(batch);
                if port.post_message(&arr).is_ok() {
                    return;
                }
            }
        }
        STATE.with(|s| s.borrow_mut().overruns += batch.len() as u64);
    });
}

/// Push the batch into the staging ring (the `ScriptProcessorNode` consumer,
/// or the pre-attach buffer). Capped — excess is dropped as overruns.
fn push_to_staging(batch: &[f32]) {
    STAGING.with(|ring| {
        let mut ring = ring.borrow_mut();
        let mut dropped = 0u64;
        for &s in batch {
            if ring.len() < AUDIO_RING_CAP {
                ring.push_back(s);
            } else {
                dropped += 1;
            }
        }
        if dropped > 0 {
            STATE.with(|s| s.borrow_mut().overruns += dropped);
        }
    });
}

/// Clear buffered samples on ROM (re)load so stale audio from the previous
/// cartridge doesn't bleed into the new one. Resets the DRC + counters.
pub fn clear_ring() {
    STAGING.with(|ring| ring.borrow_mut().clear());
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.occupancy = 0;
        s.underruns = 0;
        s.overruns = 0;
        if s.drc.is_some() {
            s.drc = Some(HermiteResampler::new());
        }
    });
}

/// Audio-queue health for the debugger Performance panel (Phase 6 wiring):
/// `(queued_samples, sample_rate, underruns, overrun_dropped)`. Zeroed until
/// [`ensure_audio`] runs.
#[must_use]
pub fn audio_health() -> (usize, u32, u64, u64) {
    STATE.with(|s| {
        let s = s.borrow();
        (s.occupancy, s.sample_rate, s.underruns, s.overruns)
    })
}

/// `console.log` shim (the `console` namespace needs no web-sys feature).
fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

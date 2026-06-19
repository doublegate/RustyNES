# 21. File System Access API for browser saves, with a download fallback

Date: 2026-06-19

## Status

Accepted (v1.7.0 "Forge", Workstream H6 — web/wasm parity).

## Context

In the browser the native `rfd` "Save As" dialog is unavailable, so RustyNES
has saved everything the user exports (TAS `.rnm` movies today) via a synthetic
`<a download>` anchor (`wasm_io::download_bytes`): mint a `Blob` object URL,
click a detached anchor, revoke the URL. That works everywhere but the file
always lands in the browser's Downloads folder with the suggested name — the
user can't choose a location, and re-exporting the same artifact silently
accumulates `rustynes-movie (1).rnm`, `(2)`, … duplicates.

The modern **File System Access API** (`window.showSaveFilePicker` →
`FileSystemFileHandle.createWritable` → `write` → `close`) gives a real
"Save As" dialog with a user-chosen path and an overwrite affordance. It is
implemented in Chromium-family browsers but **not** in Firefox or Safari
(as of this writing), so it cannot be the only path.

Two implementation wrinkles:

1. **web-sys gating.** `web-sys`'s bindings for `FileSystemFileHandle` /
   `FileSystemWritableFileStream` are behind the `web_sys_unstable_apis` cfg,
   which would mean threading a `RUSTFLAGS=--cfg=web_sys_unstable_apis` through
   the workspace, CI, and the Trunk build — a cross-cutting change that also
   risks the wasm-bindgen pin discipline.
2. **Capability detection.** The chosen path must be picked per-browser at
   runtime, not per-build.

## Decision

Add `wasm_io::save_file_with_fallback(suggested_name, description, ext, mime,
bytes)`:

- **Feature-detect** `window.showSaveFilePicker` at runtime
  (`fs_access_supported`). When absent, fall straight back to
  `download_bytes` — behaviourally identical to the pre-H6 path.
- When present, drive the API on the microtask queue (`spawn_local`) and reach
  the whole `showSaveFilePicker` / `createWritable` / `write` / `close` chain
  **dynamically through `js_sys::Reflect` + `js_sys::Function`**, so the build
  needs **no** `web_sys_unstable_apis` flag. The wasm-bindgen pin and CI
  surface are unchanged.
- On **any** FS-Access failure — including the user cancelling the picker — log
  and stop. We deliberately do **not** fall back to a download on cancel (that
  would surprise the user with an unwanted file); the download fallback is only
  taken when the API is *absent*, decided up front.

The TAS `.rnm` export (the winit `App` F6 handler + the canvas-embed F6 handler)
now routes through this helper. The save-state path stays on IndexedDB (no disk
export today); screenshots are native-only.

## Consequences

- Chromium users get a real "Save As" dialog with overwrite; Firefox/Safari
  users get exactly today's download behaviour. No build flag, no web-sys
  unstable surface, no wasm-bindgen pin churn.
- The dynamic-reflection call site is more verbose than typed bindings and is
  not compile-checked against the API shape, but the API is tiny + stable and
  every step degrades to a logged error rather than a panic (the established
  wasm I/O style).
- Native is entirely unaffected (the module is `#[cfg(target_arch = "wasm32")]`)
  and the deterministic core is untouched — AccuracyCoin holds 100% (139/139).

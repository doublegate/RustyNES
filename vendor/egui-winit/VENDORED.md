# Vendored: `egui-winit` 0.36.2, patched

Upstream: <https://github.com/emilk/egui> (`crates/egui-winit/`), crates.io
`egui-winit` 0.36.2 (tag `0.36.2`, commit `2ec7a836`). License: MIT OR
Apache-2.0; `LICENSE-MIT` and `LICENSE-APACHE` in this directory are copied
from the upstream repository at that tag, because the published package ships
neither. Every other file is byte-for-byte the crates.io 0.36.2 package.

Wired in through `[patch.crates-io]` in the workspace `Cargo.toml`, and kept out
of the workspace (`exclude`) so this project's lints and formatting do not
apply to code it did not write.

## Why (maintainer instruction, 2026-09-28)

The maintainer asked for every dependency bump, major and minor, in one PR.
egui 0.36 requires wgpu 30 (`egui-wgpu` 0.36 depends on it), so the two move
together, and `egui-winit` 0.36.2 -- the newest release -- does not compile for
`wasm32-unknown-unknown`, which RustyNES ships as its web demo:

    error[E0407]: method `bytes` is not a member of trait `egui::DroppedFile`

`egui` declares `DroppedFile::bytes` only for non-wasm targets, while
`egui-winit`'s `NativeFile` implements it unconditionally. Re-measured on
2026-09-28 by compiling a throwaway crate with this project's feature set
(`default-features = false`, `links`/`wayland`/`x11`) against the registry
0.36.2. Upstream fixed it on `main` (PR #8516, merged 2026-09-07) but has not
published a release carrying the fix.

On 2026-09-18 the project chose to WAIT for that release rather than depend on
upstream's git revision, because `deny.toml` denies git sources
(`unknown-git = "deny"`). A vendored path patch is the same mechanism already
used for `rust-libretro-sys`, keeps that policy intact, and is removed the day a
fixed `egui-winit` is published.

## The patch

Three sites in `src/lib.rs`, each marked `RustyNES vendored patch`, and nothing
else differs from the crates.io package:

- `mod dropped_file;` is compiled only for `not(target_arch = "wasm32")`.
- So is `use dropped_file::NativeFile;`.
- The `WindowEvent::DroppedFile(path)` arm pushes a `NativeFile` only on
  non-wasm targets. winit delivers no path-based dropped file on the web, and
  `NativeFile` exists only where `DroppedFile::bytes` does.

Native behaviour is unchanged: on every non-wasm target the three gated items
compile exactly as upstream wrote them.

## Unsafe code

The package has two `unsafe` blocks, both upstream's and left as written:
adding this project's `// SAFETY:` comments would break the byte-for-byte
promise above. Neither is compiled into any RustyNES build (checked
2026-09-28 with `cargo tree -e features -i egui-winit`):

- `src/clipboard.rs`, `smithay_clipboard::Clipboard::new` on the Wayland
  display pointer, exists only with the `clipboard` feature. RustyNES builds
  with `default-features = false` and enables `links`, `wayland` and `x11`,
  not `clipboard`.
- `src/safe_area.rs` is `#[cfg(target_os = "ios")]`, and the iOS app does not
  use egui-winit (it is SwiftUI over `rustynes-ios`).

If either becomes reachable, for example by enabling `clipboard`, review it
then, not before.

## Removing it

When crates.io carries an `egui-winit` newer than 0.36.2 that compiles for
wasm32 with this feature set: delete this directory, the `[patch.crates-io]`
line and the `exclude` entry, set the workspace requirement to that version,
and re-run both wasm clippy gates (default `wasm-winit`, and
`--no-default-features --features wasm-canvas`).

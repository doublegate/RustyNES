# Vendored: `rust-libretro-sys` 0.3.2, patched

Upstream: <https://github.com/max-m/rust-libretro> (`rust-libretro-sys/`),
crates.io `rust-libretro-sys` 0.3.2 (2023-02-27, the last release; the
repository has not been pushed since). License: MIT, `LICENSE` in this
directory, copied from the upstream repository. `libretro.h` and
`libretro_vulkan.h` carry their own MIT notices. The `vulkan/` headers
(`vulkan.h`, `vulkan_core.h`, `vk_platform.h`) are the Khronos Vulkan headers
under Apache-2.0, with each file's notice intact; `NOTICE` at the repository
root credits them.

Wired in through `[patch.crates-io]` in the workspace `Cargo.toml`, and kept
out of the workspace (`exclude`) so this project's lints and formatting do not
apply to code it did not write.

## The patch (RustyNES v2.8.0, maintainer decision 2026-09-25)

The generated bindings reduce `struct retro_game_info` to the opaque
`pub struct retro_game_info { pub _address: u8 }`. `rust-libretro`'s
`retro_load_game` passes `Some(*game)` to `Core::on_load_game`, so every core
received one meaningless byte instead of the frontend's `path`, `data`, `size`
and `meta`, and could load only through `RETRO_ENVIRONMENT_GET_GAME_INFO_EXT`
(which RustyNES did, for exactly this reason: `docs/libretro/WALKTHROUGH.md`).
A frontend that does not answer that command could not load a game at all
(libretro audit L-1.2).

Two changes, and nothing else differs from the crates.io 0.3.2 sources:

- `build.rs`: `.blocklist_type("retro_game_info")`.
- `src/lib.rs`: a hand-written `#[repr(C)] retro_game_info` with the header's
  four fields, in order.

Why the generated struct is opaque was not established; the header's
definition is complete and unconditional. The hand-written struct makes the
answer irrelevant to correctness.

Files dropped from the crates.io package: `Cargo.toml.orig` and cargo's
`.cargo_vcs_info.json` / `.cargo-ok` markers.

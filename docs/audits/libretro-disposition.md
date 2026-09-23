# Libretro audit — disposition ledger

Report: [`libretro-audit-report.md`](libretro-audit-report.md). Verdict vocabulary
and the closing procedure: [`README.md`](README.md). Target release per
[ADR 0041](../adr/0041-hardware-release-is-v3.0.0.md).

**Upstream timing (maintainer decision, 2026-09-22):** `.info` changes land in this
repository in v2.8.1; the upstream `libretro-super` sync stays deferred to v3.0.0,
as it has been since the MiSTer core became the priority.

| Id | Finding (report §) | Verdict | Evidence | Release | PR |
| --- | --- | --- | --- | --- | --- |
| L-1.1 | Panics unwind across `extern "C"`; `.expect` on the run path; no `catch_unwind` (§1.1, Fix 9) | UNTRIAGED | Includes 26 `panic!` sites the report attributes to `rust-libretro` 0.3.2 itself, which containment at our layer cannot reach: needs a vendor-patch vs upstream-PR assessment | v2.8.0 | |
| L-1.2 | `on_load_game` ignores `_game` and requires `GET_GAME_INFO_EXT` (§1.2, Fix 10) | CONFIRMED | `rustynes-libretro/src/lib.rs:855` takes `_game`; `:899` `.ok_or("Frontend does not support get_game_info_ext")?` with no fallback | v2.8.0 | |
| L-1.3 | Memory maps not deregistered on unload (§1.3, Fix 5) | UNTRIAGED | | v2.8.0 | |
| L-1.4 | `static mut` instance, no teardown, repeated-init panic in `rust-libretro` (§1.4) | UNTRIAGED | Upstream-crate finding; assess with L-1.1 | v2.8.0 | |
| L-1.5 | `*const` cast to `&mut` in `retro_unserialize`; null/NUL hazards in disk labels (§1.5) | UNTRIAGED | Upstream-crate finding, except the NUL termination of our own labels | v2.8.0 | |
| L-2.1 | `serialize_size` fixed at load; attaching a Zapper grows the snapshot (§2.1, Fix 3) | CONFIRMED | `serialize_size` assigned only at `lib.rs:971/979`; `set_zapper` attaches the device lazily (`nes.rs:1598-1603`); `bus_snapshot.rs:162-167` then writes 6 extra bytes; `on_serialize` returns false once the slice is short | v2.8.0 | |
| L-2.2 | `SectionIter` rejects host trailing padding (§2.2, Fix 4) | UNTRIAGED | | v2.8.0 | |
| L-2.3 | ~244 KiB PPU snapshot allocation per `retro_serialize` (§2.3) | UNTRIAGED | Performance: >3% A/B + byte-identical, or recorded rejected | v2.8.1 | |
| L-2.4 | `internal_data_bus` not serialised (§2.4) | CONFIRMED | Field at `rustynes-core/src/bus.rs:612`, read for `$4015` bit 5 (`:3476,3930`); absent from `bus_snapshot.rs`, which serialises only `open_bus` (`:46`). Fix is an additive snapshot section under ADR 0028 rules | v2.8.0 | |
| L-2.5 | Joypad polled with 16 FFI calls per port; redundant `poll_input` (§2.5, Fix 1) | UNTRIAGED | | v2.8.1 | |
| L-2.6 | Two-pass RGBA→XRGB blit; redundant zero-fill in `compose_dual` (§2.6, Fix 2) | UNTRIAGED | | v2.8.1 | |
| L-2.7 | Scalar audio `push`; no audio buffer-status callback (§2.7) | UNTRIAGED | | v2.8.1 | |
| L-3.1a | `make PREFIX=/usr/local` overrides the library-prefix variable (§3.1) | PARTIAL | `Makefile:95,98` `PREFIX :=` is used as the library-file prefix, and the Windows empty value is correct (cargo adds no `lib` to DLLs). The report's actual claim, a command-line override, is untested | v2.8.1 | |
| L-3.1b | `platform=win` vs `windows`; `DEBUG=1` ignored; no cross `AR`; hardcoded `TARGET_DIR` (§3.1) | UNTRIAGED | | v2.8.1 | |
| L-3.2 | Missing ARM / Raspberry Pi / Emscripten / console targets (§3.2) | UNTRIAGED | Scope question, not a defect; each added target needs a buildable CI job | v2.8.1 | |
| L-3.3 | `__retro_init_core` leaked; no symbol version script (§3.3) | UNTRIAGED | | v2.8.1 | |
| L-3.4a | `.info` lacks `visible = "true"` (§3.4) | PARTIAL | The key is absent; the report's claim that it is mandatory is unverified. Added only if `libretro-super` documentation requires it | v2.8.1 | |
| L-3.4b | UNIF (`unf\|unif`) missing from `supported_extensions` (§3.4) | CONFIRMED | `.info` line 4 and `lib.rs:694` declare `nes\|fds`; `rustynes-mappers/src/lib.rs:309` parses UNIF | v2.8.1 | |
| L-3.5a | `scripts/resubmit_libretro_docs_pr.sh` missing a line continuation (§3.5, Fix 8) | CONFIRMED | Line 27 `--base master` has no trailing backslash, so `--title` runs as a separate command | v2.8.1 | |
| L-3.5b | `docs/libretro/` stale (§3.5) | UNTRIAGED | | v2.8.1 | |
| L-S1 | Only port 0 input descriptors; Four Score truncated; no turbo/SOCD; Zapper left on the bus; `CoreOptions` empty (executive summary only) | UNTRIAGED | Summary-table claims with no body section in the report; verify each before planning work | v2.8.1 | |

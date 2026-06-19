#!/usr/bin/env bash
# build_rcheevos_wasm.sh — the casual-mode browser RetroAchievements build track.
#
# v1.5.0 "Lens" Workstream G (ADR 0015). Compiles the vendored RetroAchievements
# `rcheevos` C library (the SAME sources the native `rustynes-cheevos` build.rs
# links via `cc`) to WebAssembly with Emscripten, producing a SIDE-LOADED module
# (`rcheevos.wasm` + `rcheevos.js` loader glue) that the browser frontend calls
# through `wasm_cheevos.rs`'s `#[wasm_bindgen]` bridge.
#
# WHY A SEPARATE ARTIFACT (not linked into the Rust `.wasm`):
#   trunk builds the Rust frontend for `wasm32-unknown-unknown`. Emscripten emits
#   `wasm32-unknown-emscripten` objects with a DIFFERENT ABI + libc + linking
#   model; you cannot `cc`-link an emscripten `.a` into a `wasm32-unknown-unknown`
#   cdylib. The honest architecture (anticipated in ADR 0015) is therefore a
#   second build track: rcheevos becomes its own Emscripten module loaded as JS
#   glue alongside the Rust `.wasm`, and the Rust side talks to it via the
#   `RCHEEVOS_GLUE` extern bridge. This script produces that module.
#
# WHAT THIS DOES NOT DO (maintainer-manual, no headless path — ADR 0015):
#   - It does NOT deploy the auth proxy (see docs/cheevos-browser.md §Auth proxy
#     contract + scripts/cheevos/auth-proxy.example.toml).
#   - It does NOT verify achievements unlock against a live RA account in a
#     browser (requires a real login + the deployed proxy + a human).
#
# OUTPUT (gitignored — `*.wasm` / `*.a`; rebuild on demand, never committed):
#   crates/rustynes-frontend/web/cheevos/rcheevos.wasm
#   crates/rustynes-frontend/web/cheevos/rcheevos.js
#
# PREREQS:
#   Emscripten on PATH. On this dev box it is at /usr/lib/emscripten (NOT on PATH
#   by default):  export PATH=/usr/lib/emscripten:$PATH
#   Verify:       emcc --version
#
# USAGE:
#   export PATH=/usr/lib/emscripten:$PATH
#   ./scripts/cheevos/build_rcheevos_wasm.sh
#
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
vendor="$repo/crates/rustynes-cheevos/vendor/rcheevos"
out="$repo/crates/rustynes-frontend/web/cheevos"

if ! command -v emcc >/dev/null 2>&1; then
  echo "error: emcc not found on PATH." >&2
  echo "       Install Emscripten and run: export PATH=/usr/lib/emscripten:\$PATH" >&2
  exit 1
fi

echo "== emcc =="
emcc --version | head -1

# The SAME compile defines + excluded-source list as the native build.rs, so the
# wasm rcheevos behaves identically to the native one (no disc / zip / encrypted /
# Lua paths; static archive; hash-from-bytes load).
DEFINES=(
  -DRC_STATIC
  -DRC_CLIENT_SUPPORTS_HASH
  -DRC_DISABLE_LUA
  -DRC_HASH_NO_DISC
  -DRC_HASH_NO_ENCRYPTED
  -DRC_HASH_NO_ZIP
)
EXCLUDE=(
  hash_disc.c hash_zip.c hash_encrypted.c cdreader.c aes.c
  rc_libretro.c rc_client_external.c rc_client_raintegration.c
)

# Build the find(1) prune expression from EXCLUDE.
prune=()
for f in "${EXCLUDE[@]}"; do prune+=( ! -name "$f" ); done

mapfile -t SRCS < <(find "$vendor/src" -name '*.c' "${prune[@]}" | sort)
if [ "${#SRCS[@]}" -eq 0 ]; then
  echo "error: no rcheevos .c sources found under $vendor/src" >&2
  exit 1
fi
echo "== compiling ${#SRCS[@]} rcheevos translation units to wasm =="

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
objs=()
for f in "${SRCS[@]}"; do
  obj="$work/$(echo "$f" | tr '/' '_').o"
  emcc -c "$f" -I"$vendor/include" -I"$vendor/src" "${DEFINES[@]}" -O2 -o "$obj"
  objs+=( "$obj" )
done

emar rcs "$work/librcheevos.a" "${objs[@]}"
echo "== static archive: $(du -h "$work/librcheevos.a" | cut -f1) =="

mkdir -p "$out"

# The rc_client surface the browser bridge needs. Mirrors the FFI subset the
# native `crates/rustynes-cheevos/src/ffi.rs` declares — MINUS the hardcore
# toggle. Casual-only is STRUCTURAL: `_rc_client_set_hardcore_enabled` is
# deliberately NOT exported, so DevTools cannot reach it through this module.
EXPORTS='[
  "_rc_client_create","_rc_client_destroy",
  "_rc_client_set_event_handler",
  "_rc_client_set_unofficial_enabled",
  "_rc_client_begin_login_with_password","_rc_client_begin_login_with_token",
  "_rc_client_logout","_rc_client_get_user_info",
  "_rc_client_begin_identify_and_load_game","_rc_client_unload_game",
  "_rc_client_do_frame","_rc_client_idle","_rc_client_reset",
  "_rc_client_get_rich_presence_message",
  "_rc_client_create_achievement_list","_rc_client_destroy_achievement_list",
  "_rc_client_get_user_game_summary",
  "_rc_client_progress_size","_rc_client_serialize_progress_sized",
  "_rc_client_deserialize_progress",
  "_rc_version_string",
  "_malloc","_free"
]'

emcc "$work/librcheevos.a" -O3 \
  -sEXPORTED_FUNCTIONS="$EXPORTS" \
  -sEXPORTED_RUNTIME_METHODS='["ccall","cwrap","addFunction","removeFunction","UTF8ToString","stringToUTF8","lengthBytesUTF8","getValue","setValue","HEAPU8"]' \
  -sALLOW_TABLE_GROWTH=1 \
  -sALLOW_MEMORY_GROWTH=1 \
  -sMODULARIZE=1 \
  -sEXPORT_NAME=createRcheevosModule \
  -sENVIRONMENT=web \
  -o "$out/rcheevos.js"

echo "== output =="
ls -l "$out/rcheevos.js" "$out/rcheevos.wasm"
echo
echo "Built the casual-only browser rcheevos module."
echo "Next (maintainer-manual, ADR 0015): deploy the auth proxy"
echo "(scripts/cheevos/auth-proxy.example.toml + docs/cheevos-browser.md) and"
echo "live-verify a casual unlock with a real RA account in a browser."

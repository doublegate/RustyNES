#!/usr/bin/env bash
# The `platform` paths-filter in ci.yml must list everything that compiles or
# links DIFFERENTLY on macOS and Windows. This derives that set from the tree
# and fails if the filter misses any of it.
#
# WHY (2026-11-12). `full-build` (the only macOS / Windows build on a regular
# PR) runs only when that filter matches, and a skipped job passes `CI success`,
# `main`'s only required check. So a file missing from the list is a gate that
# silently stops running for a change to it -- the shape that hid twelve
# workspace crates from android.yml's filter until 2026-09-23. A hand-written
# list of `#[cfg(windows)]` files is exactly the kind that goes stale: the
# first hand derivation for this one missed `clock.rs`, whose `unix` sits
# inside a `cfg(all(...))`.
#
# WHAT IT CHECKS, for every crate `cargo tree -p rustynes-frontend --features
# full -e normal,build --target all` reaches (the build `full-build` runs;
# `--target all`, or a target-specific dependency is invisible):
#   1. the crate's Cargo.toml is matched (a feature change to a dependency such
#      as `windows-sys` moves no Cargo.lock line);
#   2. a crate with a build.rs is matched WHOLE (`crates/<name>/**`), since its
#      build script compiles C or probes the target;
#   3. every `src/**/*.rs` file mentioning target_os / target_vendor /
#      target_family, or a `cfg(...)` naming `windows` or `unix`, is matched;
#   4. the filter carries the `*shared` alias (lockfile, toolchain, `.cargo/`,
#      CI plumbing);
#   5. every path the filter names exists, so the list keeps describing the tree.
# A check that examines nothing passes nothing: it fails closed when the
# filter, the crate list or the file scan comes back empty.
#
# Matching is literal or a trailing `/**`; those are the only two forms the
# list uses, and anything else is refused rather than guessed at.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
# Overridable only so the audit's own mutation tests can point it at a copy.
ci="${PLATFORM_FILTER_WORKFLOW:-.github/workflows/ci.yml}"

# The block under `platform:` (a key with no value on its line, unlike the job
# output `platform: ${{ ... }}`), up to the first line indented no deeper.
mapfile -t entries < <(awk '
  !inblk && /^[[:space:]]*platform:[[:space:]]*$/ { inblk = 1; ind = match($0, /[^ ]/); next }
  inblk {
    if ($0 ~ /^[[:space:]]*(#.*)?$/) next
    if (match($0, /[^ ]/) <= ind) exit
    if ($0 ~ /^[[:space:]]*- /) { v = $0; sub(/^[[:space:]]*- /, "", v); gsub(/'"'"'/, "", v); print v }
  }' "$ci")

if [ "${#entries[@]}" -eq 0 ]; then
  echo "::error file=$ci::no 'platform:' filter found in $ci; this audit has nothing to check"
  exit 1
fi

shared=0
patterns=()
fail=0
for e in "${entries[@]}"; do
  case "$e" in
    '*shared') shared=1 ;;
    *'/**') [ -d "${e%/**}" ] || { echo "::error file=$ci::platform filter names '$e', which is not a directory"; fail=1; }
            patterns+=("$e") ;;
    *'*'*)  echo "::error file=$ci::platform filter entry '$e' is neither a literal path nor a trailing '/**'"; fail=1 ;;
    *)      [ -f "$e" ] || { echo "::error file=$ci::platform filter names '$e', which does not exist"; fail=1; }
            patterns+=("$e") ;;
  esac
done
[ "$shared" -eq 1 ] || { echo "::error file=$ci::the platform filter must include '*shared'"; fail=1; }

matched() {
  local path="$1" p
  for p in "${patterns[@]}"; do
    case "$p" in
      *'/**') [[ "$path" == "${p%/**}/"* ]] && return 0 ;;
      *) [ "$path" = "$p" ] && return 0 ;;
    esac
  done
  return 1
}

root="$PWD"
mapfile -t crates < <(
  cargo tree -p rustynes-frontend --features full -e normal,build --target all \
    --prefix none --format '{p}' \
  | grep -oE "\(${root}/[^)]+\)" | tr -d '()' | sed "s#^${root}/##" | sort -u
)
if [ "${#crates[@]}" -eq 0 ]; then
  echo "::error::cargo tree listed no workspace crate for the full build; this audit has nothing to check"
  exit 1
fi

# An OS predicate inside a cfg attribute, cfg_attr or cfg! macro, or any
# target_os / target_vendor / target_family token.
OS_CFG='target_os|target_vendor|target_family|cfg(_attr)?!?\s*\([^\]]{0,400}?\b(windows|unix)\b'
# The name of a module whose declaration sits under such a cfg.
OS_MOD='#\[cfg(_attr)?\s*\([^\]]{0,400}?\b(windows|unix|target_os|target_vendor|target_family)\b[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+\K[A-Za-z0-9_]+(?=\s*;)'
scanned=0
for c in "${crates[@]}"; do
  matched "$c/Cargo.toml" || { echo "::error file=$ci::platform filter misses '$c/Cargo.toml' (a crate of the full build)"; fail=1; }
  if [ -f "$c/build.rs" ] && ! printf '%s\n' "${patterns[@]}" | grep -qxF "$c/**"; then
    echo "::error file=$ci::platform filter must match '$c/**' whole: the crate has a build.rs"
    fail=1
  fi
  [ -d "$c/src" ] || continue
  # Whole-file matching (`-z` reads each file as one record), because rustfmt
  # wraps a long `#[cfg(all(...))]` across lines and a line-based pattern then
  # sees `cfg(` and `unix,` on different lines (PR #650 review). The span after
  # `cfg(` / `cfg_attr(` / `cfg!(` is bounded at 400 characters and may not
  # cross a `]`, so it stays inside one attribute or macro call rather than
  # running on into ordinary code that happens to say "windows".
  while IFS= read -r f; do
    scanned=$((scanned + 1))
    matched "$f" || { echo "::error file=$f::OS-specific cfg in a file the platform filter in $ci does not list"; fail=1; }
  done < <(grep -rlzP "$OS_CFG" "$c/src" --include='*.rs' | sort)
  # A module gated on its `mod` declaration (`#[cfg(windows)] mod foo;`) can
  # be OS-specific with no cfg of its own inside foo.rs: the parent matches
  # the scan above, foo.rs does not. Resolve each such declaration to its file
  # and require that too.
  while IFS= read -r f; do
    case "$(basename "$f")" in
      lib.rs|main.rs|mod.rs) dir="$(dirname "$f")" ;;
      *) dir="${f%.rs}" ;;
    esac
    while IFS= read -r m; do
      [ -n "$m" ] || continue
      for cand in "$dir/$m.rs" "$dir/$m/mod.rs"; do
        [ -f "$cand" ] || continue
        scanned=$((scanned + 1))
        matched "$cand" || { echo "::error file=$cand::module '$m' is declared under an OS cfg in $f, and the platform filter in $ci does not list it"; fail=1; }
      done
    done < <(grep -ozP "$OS_MOD" "$f" | tr '\0' '\n')
  done < <(grep -rlzP "$OS_MOD" "$c/src" --include='*.rs' | sort)
done

if [ "$scanned" -eq 0 ]; then
  echo "::error::found no OS-specific source file in ${#crates[@]} crates; the scan pattern is wrong"
  exit 1
fi
if [ "$fail" -ne 0 ]; then
  echo "The platform filter in $ci is out of date; add the paths named above (and see the note on that filter)."
  exit 1
fi
echo "platform filter: ${#crates[@]} crates, ${scanned} OS-specific source files, all matched by ${#patterns[@]} entries plus *shared"

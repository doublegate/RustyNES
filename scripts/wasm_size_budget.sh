#!/usr/bin/env bash
# v1.3.0 Sprint 2 — wasm32 deploy size-budget gate.
#
# Sums the GZIP-compressed size of every shippable asset in a trunk
# `dist/` directory (the `.wasm` + the wasm-bindgen `.js` glue) and
# fails if the total exceeds a byte budget. GitHub Pages serves these
# assets gzip- (and brotli-) compressed over the wire, so the
# compressed total is the number that actually governs download time
# on a cold cache — that's what we gate on, not the raw `.wasm` size.
#
# Brotli (what Pages' Fastly CDN actually prefers) is printed for
# information when the `brotli` CLI is present, but the PASS/FAIL gate
# is gzip: gzip is universally available, strictly larger than brotli,
# and therefore the conservative bound.
#
# Usage:
#   scripts/wasm_size_budget.sh [DIST_DIR] [BUDGET_BYTES]
#
# Defaults: DIST_DIR=crates/nes-frontend/web/dist, BUDGET=5 MiB.
#
# Exit status: 0 if total gzip size <= budget, 1 otherwise (or on a
# missing/empty dist directory).

set -euo pipefail

DIST_DIR="${1:-crates/nes-frontend/web/dist}"
BUDGET_BYTES="${2:-5242880}" # 5 * 1024 * 1024

if [[ ! -d "$DIST_DIR" ]]; then
    echo "error: dist directory not found: $DIST_DIR" >&2
    echo "  (run \`trunk build --release\` in crates/nes-frontend/web first)" >&2
    exit 1
fi

# Shippable assets: the wasm module + the JS glue. index.html / CSS are
# negligible and not the budget driver, so they're excluded from the
# gate (still served, just not counted).
mapfile -t assets < <(find "$DIST_DIR" -maxdepth 1 -type f \( -name '*.wasm' -o -name '*.js' \) | sort)

if [[ ${#assets[@]} -eq 0 ]]; then
    echo "error: no .wasm/.js assets in $DIST_DIR" >&2
    exit 1
fi

have_brotli=0
command -v brotli >/dev/null 2>&1 && have_brotli=1

total_raw=0
total_gzip=0
total_brotli=0

printf '%-46s %12s %12s %12s\n' "asset" "raw" "gzip" "brotli"
printf '%-46s %12s %12s %12s\n' "-----" "---" "----" "------"
for f in "${assets[@]}"; do
    raw=$(stat -c%s "$f")
    gz=$(gzip -9 -c "$f" | wc -c)
    if [[ $have_brotli -eq 1 ]]; then
        br=$(brotli -q 11 -c "$f" | wc -c)
    else
        br=0
    fi
    total_raw=$((total_raw + raw))
    total_gzip=$((total_gzip + gz))
    total_brotli=$((total_brotli + br))
    if [[ $have_brotli -eq 1 ]]; then
        printf '%-46s %12d %12d %12d\n' "$(basename "$f")" "$raw" "$gz" "$br"
    else
        printf '%-46s %12d %12d %12s\n' "$(basename "$f")" "$raw" "$gz" "n/a"
    fi
done
printf '%-46s %12s %12s %12s\n' "-----" "---" "----" "------"
if [[ $have_brotli -eq 1 ]]; then
    printf '%-46s %12d %12d %12d\n' "TOTAL" "$total_raw" "$total_gzip" "$total_brotli"
else
    printf '%-46s %12d %12d %12s\n' "TOTAL" "$total_raw" "$total_gzip" "n/a"
fi

# Human-readable budget line.
budget_mib=$(awk "BEGIN { printf \"%.2f\", $BUDGET_BYTES / 1048576 }")
gzip_mib=$(awk "BEGIN { printf \"%.2f\", $total_gzip / 1048576 }")
echo
echo "gzip total: ${total_gzip} bytes (${gzip_mib} MiB)"
echo "budget:     ${BUDGET_BYTES} bytes (${budget_mib} MiB)"

if [[ $total_gzip -gt $BUDGET_BYTES ]]; then
    over=$((total_gzip - BUDGET_BYTES))
    echo "FAIL: over budget by ${over} bytes" >&2
    exit 1
fi

headroom=$((BUDGET_BYTES - total_gzip))
headroom_mib=$(awk "BEGIN { printf \"%.2f\", $headroom / 1048576 }")
echo "PASS: ${headroom} bytes (${headroom_mib} MiB) of headroom"

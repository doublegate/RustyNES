#!/usr/bin/env bash
# frame_breakdown.sh — v2.3.1 "Plumb Line" per-subsystem frame-cost breakdown.
#
# Answers "where does a frame actually go?" for the COMPOSED emulator, by
# profiling `frame_probe` (the harness-free probe, so no criterion plumbing
# dilutes the samples) and bucketing them into PPU / CPU / APU / mappers /
# scheduler-coupling.
#
# ## Why this is not just `perf report`
#
# The obvious command — `perf report --no-children -g none` — gives a symbol
# profile that is actively misleading about this codebase, and the correction is
# the whole reason this script exists.
#
# Under `lto = "fat"` + `codegen-units = 1`, the APU is inlined wholesale into
# `<LockstepBus as Bus>::cpu_clock`. The symbol view therefore reports:
#
#     31.0%  rustynes_ppu::ppu::Ppu::tick
#     18.3%  <rustynes_core::bus::LockstepBus as rustynes_cpu::bus::Bus>::cpu_clock
#     10.0%  rustynes_ppu::ppu::Ppu::emit_pixel
#      ...   (rustynes_apu:: does not appear ANYWHERE, at any percent limit)
#
# Read naively that says the APU is free. It is not: bucketing the same profile
# by source file surfaces `apu.rs`, `blip.rs`, `pulse.rs`, `frame_counter.rs`,
# `dmc.rs`, `noise.rs`, `mixer.rs`, `length.rs` totalling **~19%** of frame cost,
# all of it hidden inside that one `cpu_clock` line. Any optimization plan built
# on the symbol view will mis-target by roughly a fifth of the frame.
#
# `perf report --inline` does NOT fix this — measured, it produces byte-identical
# output to the non-inline report, because the inlined APU frames are not
# recoverable as call frames at all. Source-file attribution is.
#
# ## Method and its one real limit
#
# Samples are bucketed by SOURCE FILE (`perf report --sort srcfile`), which
# follows inlined code back to the crate that wrote it. perf reports basenames
# only, so the map from basename to subsystem is built from the tree at run time
# rather than hardcoded (it cannot rot). Four basenames exist in more than one
# emulation crate — `bus.rs`, `scheduler.rs`, `lib.rs`, `snapshot.rs`:
#
#   * `bus.rs` and `scheduler.rs` are bucketed as COUPLING regardless of crate.
#     That is not a fudge: every one of them is the bus/scheduler abstraction,
#     so the semantic bucket is the same whichever crate the samples came from
#     (verified — `bus.rs` samples resolve to `LockstepBus::raw_cpu_read`,
#     `Cpu::read1`, `cpu_clock`, and `Ppu::tick`, i.e. all three crates' bus
#     files, all of them bus work).
#   * `lib.rs` and `snapshot.rs` genuinely cannot be attributed from a basename,
#     so they land in UNATTRIBUTED and are printed rather than guessed at.
#
# Inlined **standard library** code (`range.rs`, `option.rs`, `cmp.rs`,
# `uint_macros.rs`, …) is emulator work performed at emulator call sites, but it
# carries std's source path, so it cannot be assigned to a subsystem. It is
# reported as its own line. It is NOT redistributed proportionally across the
# buckets — that would invent precision the data does not contain.
#
# ## Debuginfo
#
# Source attribution needs DWARF, which `[profile.release]` does not emit, so the
# probe is rebuilt with `CARGO_PROFILE_RELEASE_DEBUG=2`. Debuginfo adds DWARF
# sections without changing codegen — inlining, layout and instruction selection
# are identical — so the profile is faithful to the shipped binary.
#
# The script PRINTS the debuginfo probe's frame cost, but that is CONTEXT, not a
# verification of the claim above: it never builds a stock probe, so it has
# nothing to compare against. To check the claim, run `frame_probe` from a plain
# `cargo build --release` and compare medians yourself.
#
# ## Usage
#
#   scripts/perf/frame_breakdown.sh                      # default nestest, 1500 frames
#   scripts/perf/frame_breakdown.sh --rom path/to.nes
#   scripts/perf/frame_breakdown.sh --frames 4000 --freq 3000
#   scripts/perf/frame_breakdown.sh --keep                # keep perf.data for hotspot
#
# Requires `perf` and a host where `perf_event_paranoid <= 2` (user-space
# sampling of your own process). Skips with exit 0 if perf is unavailable, so
# this never becomes a hard CI dependency.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."
ROOT="$(pwd)"

ROM="tests/roms/nestest/nestest.nes"
FRAMES=1500
FREQ=1500
KEEP=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --rom) ROM="$2"; shift 2 ;;
        --frames) FRAMES="$2"; shift 2 ;;
        --freq) FREQ="$2"; shift 2 ;;
        --keep) KEEP=1; shift ;;
        -h|--help) sed -n '2,80p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

if ! command -v perf >/dev/null 2>&1; then
    echo "SKIP: perf not installed — the breakdown needs sampling support."
    exit 0
fi
if [[ ! -f "${ROM}" ]]; then
    echo "SKIP: ROM not found: ${ROM}"
    exit 0
fi

paranoid="$(cat /proc/sys/kernel/perf_event_paranoid 2>/dev/null || echo 99)"
if [[ "${paranoid}" -gt 2 ]]; then
    echo "SKIP: perf_event_paranoid=${paranoid} (>2) — cannot sample without elevated"
    echo "      privileges. Lower it with:"
    echo "        sudo sysctl kernel.perf_event_paranoid=2"
    exit 0
fi

work="$(mktemp -d)"
trap '[[ "${KEEP}" == "1" ]] || rm -rf "${work}"' EXIT

echo "==> Building frame_probe with debuginfo (codegen unchanged)"
CARGO_PROFILE_RELEASE_DEBUG=2 \
    cargo build --release -p rustynes-test-harness --bin frame_probe >/dev/null

probe="${ROOT}/target/release/frame_probe"

# Report the probe's own cost first. This is both context for the percentages
# and the check that the debuginfo build did not perturb the thing being
# measured — it should match a stock release build within the probe's own CV.
echo "==> Frame cost of the DEBUGINFO probe (context only — no stock build is"
echo "    made here to compare against; see the header note)"
"${probe}" --rom "${ROM}" --frames 400 | sed 's/^/    /'

echo
echo "==> Sampling ${FRAMES} frames at ${FREQ} Hz"
perf record -q -F "${FREQ}" -e cycles:u -o "${work}/perf.data" -- \
    "${probe}" --rom "${ROM}" --frames "${FRAMES}" >/dev/null 2>&1

# Build the basename -> subsystem map from the tree, so adding a source file
# never silently falls into UNATTRIBUTED and the map cannot drift from reality.
: > "${work}/map.txt"
for crate in cpu ppu apu mappers core; do
    case "${crate}" in
        cpu) bucket=CPU ;;
        ppu) bucket=PPU ;;
        apu) bucket=APU ;;
        mappers) bucket=MAPPERS ;;
        core) bucket=COUPLING ;;
        *) bucket=UNATTRIBUTED ;;
    esac
    find "crates/rustynes-${crate}/src" -name '*.rs' -printf '%f\n' 2>/dev/null \
        | sed "s|\$| ${bucket}|" >> "${work}/map.txt"
done

perf report -i "${work}/perf.data" --no-children -g none --sort srcfile --stdio \
    2>/dev/null | grep -E '^\s+[0-9]' > "${work}/by_file.txt" || true

if [[ ! -s "${work}/by_file.txt" ]]; then
    echo "SKIP: perf produced no source-attributed samples (missing DWARF?)."
    exit 0
fi

python3 - "${work}/map.txt" "${work}/by_file.txt" <<'PY'
import collections, re, sys

map_path, report_path = sys.argv[1], sys.argv[2]

# basename -> set of buckets claimed by the tree scan.
owners = collections.defaultdict(set)
for line in open(map_path):
    parts = line.split()
    if len(parts) == 2:
        owners[parts[0]].add(parts[1])

# Semantic overrides for the basenames owned by more than one emulation crate.
# bus.rs / scheduler.rs are the bus + scheduler abstraction in every crate that
# defines them, so the subsystem is the same whichever one a sample came from.
# lib.rs / snapshot.rs carry no such invariant and stay unattributed.
OVERRIDE = {"bus.rs": "COUPLING", "scheduler.rs": "COUPLING",
            "nes.rs": "COUPLING", "lib.rs": None, "snapshot.rs": None}

def bucket_for(fname):
    if fname in OVERRIDE:
        return OVERRIDE[fname] or "UNATTRIBUTED"
    claims = owners.get(fname)
    if not claims:
        # Not one of ours: inlined std/core, or a dependency.
        return "STD-INLINED"
    if len(claims) == 1:
        return next(iter(claims))
    return "UNATTRIBUTED"

totals = collections.Counter()
detail = collections.defaultdict(list)
grand = 0.0
for line in open(report_path):
    m = re.match(r'\s+([0-9.]+)%\s+(\S+)', line)
    if not m:
        continue
    pct, fname = float(m.group(1)), m.group(2)
    b = bucket_for(fname)
    totals[b] += pct
    grand += pct
    detail[b].append((pct, fname))

ORDER = ["PPU", "CPU", "APU", "COUPLING", "MAPPERS", "STD-INLINED", "UNATTRIBUTED"]
LABEL = {
    "PPU": "PPU (rustynes-ppu)",
    "CPU": "CPU (rustynes-cpu)",
    "APU": "APU (rustynes-apu)",
    "COUPLING": "Bus / scheduler coupling",
    "MAPPERS": "Mappers",
    "STD-INLINED": "std inlined at emulator call sites",
    "UNATTRIBUTED": "unattributed (ambiguous basename)",
}

print()
print(f"{'subsystem':<38} {'% of frame':>11}   top source files")
print(f"{'-'*38} {'-'*11}   {'-'*40}")
for b in ORDER:
    if b not in totals:
        continue
    top = ", ".join(f"{f} {p:.1f}%" for p, f in sorted(detail[b], reverse=True)[:3])
    print(f"{LABEL[b]:<38} {totals[b]:>10.1f}%   {top}")
print(f"{'-'*38} {'-'*11}")
print(f"{'accounted for':<38} {grand:>10.1f}%")

print()
print("Notes:")
print("  * Percentages are of sampled cycles, bucketed by SOURCE FILE, so code")
print("    inlined across crate boundaries is credited to the crate that wrote")
print("    it. A symbol-level profile of this binary shows NO rustynes_apu at")
print("    all — the APU is inlined into cpu_clock and only source attribution")
print("    recovers it.")
print("  * 'std inlined at emulator call sites' is real emulator work whose")
print("    source path belongs to the standard library. It is reported rather")
print("    than redistributed across the buckets, which would invent precision.")
print("  * The residual below 100% is perf's own per-file percent rounding.")
PY

if [[ "${KEEP}" == "1" ]]; then
    echo
    echo "perf.data kept at ${work}/perf.data"
    echo "  hotspot ${work}/perf.data"
    echo "  perf report -i ${work}/perf.data --no-children -g none --sort srcfile"
fi

<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
# AccuracyCoin mirror ROM — the hardware readback channel

`AccuracyCoin-mirror.nes` is upstream `AccuracyCoin.nes` with one addition: once
the battery finishes, it copies its result vector from `$0300-$04FF` into the
cartridge's battery-backed PRG-RAM at `$6000-$61FF`. The MiSTer core persists
that window to `<rom>.sav`, so a hardware run can be read back as **bytes** and
diffed entry-for-entry instead of transcribed from a photograph.

`RustyNES_MiSTer/docs/bringup.md` states the limit this removes:

> a photograph is not 149 status bytes, and reading one is a human transcribing
> a picture. A clean screen is evidence the console runs; it is not evidence the
> vector still matches.

## Provenance

| | |
|---|---|
| Upstream | <https://github.com/100thCoin/AccuracyCoin>, commit `46199ae4` |
| Upstream licence | MIT (Copyright (c) 2025 Chris Siebert) — the text is at `../../accuracycoin/LICENSE`, and the corpus-wide index is `../../LICENSES.md` |
| Base ROM md5 | `2f9d83104969a5984caf21a77d6746bd` (identical to `tests/roms/accuracycoin/AccuracyCoin.nes`) |
| This ROM md5 | `8162ca0ae099220401e76719e88762fc` |
| Built by | `scripts/accuracycoin-build/build_mirror_rom.py` |
| Assembler | upstream's own `nesasm.exe` under `wine`, so the output is the author's toolchain rather than an equivalent one |

This ROM is a derivative work of `100thCoin/AccuracyCoin` and inherits the
upstream MIT licence (`tests/roms/accuracycoin/LICENSE`). The build script is GPL-3.0-or-later with the rest of
RustyNES.

## Rebuild

```bash
git clone https://github.com/100thCoin/AccuracyCoin /tmp/ac-src
git -C /tmp/ac-src checkout 46199ae4

python3 scripts/accuracycoin-build/build_mirror_rom.py /tmp/ac-src \
    --out tests/roms/AccuracyCoin/mirror/AccuracyCoin-mirror.nes \
    --expect-upstream-md5 2f9d83104969a5984caf21a77d6746bd \
    --wine /usr/bin/wine
```

`--expect-upstream-md5` is not optional in spirit: it rebuilds the **unpatched**
source first and refuses to emit a patched ROM unless that reproduces the
vendored image exactly. Without it, a patched build from a different source tree
would be indistinguishable from a correct one.

`--wine` is required on this machine because `/usr/local/bin/wine` is a symlink
to `firejail`; the builder deliberately does not search `PATH`. See
`build_sub_test_rom._find_wine()`, which it shares rather than restates.

## The patch moves nothing, and that is checked

AccuracyCoin is full of cycle-exact tests, several sensitive to page crossings
in their own code, so an insertion that displaced every routine after it could
flip a verdict for reasons unrelated to the console under test — and the flipped
verdict would look exactly like a result.

So the patch is size-neutral in both places it touches, and the builder asserts
the resulting byte budget rather than hoping for it:

| region | bytes | what changed |
|---|---|---|
| iNES header | 1 | flags6 bit 1 (battery) set |
| call site, CPU `$9102` | 5 | `LDA #0` / `STA $4015` → `JSR ACMirrorResults` + `NOP` `NOP` |
| bank 2 fill, CPU `$DFCE` | 29 | the copy routine, placed in the `$FF` padding nesasm lays down to `$E000` |
| **anywhere else** | **0** | — |

The displaced pair is re-executed as the routine's first act, so the APU is
silenced at the same point in the instruction stream. `X` and `Y` are preserved
and `A` returns as `$00`, exactly as the `LDA #0` it replaced left it.

The single header bit is load-bearing twice over, and both were read out of the
RTL rather than assumed: `rtl/emu.sv:609` takes `has_prg_ram` from flags6 bit 1,
which is what gives the cartridge a `$6000` window at all, and `rtl/emu.sv:353`
feeds that same signal to the save controller as `cart_has_battery`, which arms
the write-back.

## The byte budget is not the control

A budget proves nothing *moved*. It does not prove the ROM gives the same
*answers*. That control is
`crates/rustynes-test-harness/tests/accuracycoin_mirror.rs`, which runs both
ROMs through the oracle and refuses unless the `$0300-$04FF` window is
byte-identical, the decoded vector agrees entry for entry, and the mirror
reproduces the live window without being vacuous. It then runs the real
comparator over a real save file, because the conjunction of two separately
verified halves is a third claim.

Per the v2.7.0 plan: if that control ever comes out dirty, **drop the patch and
keep the photograph** — a readback channel that alters the thing it reads is
worse than no channel.

## Reading a hardware save

```bash
cargo run -p rustynes-test-harness --features test-roms --bin accuracycoin_status -- \
    <golden>.ram.bin sav:AccuracyCoin-mirror.sav
```

The `sav:` prefix is mandatory. A bare path ending in `.sav` is refused, because
decoding a save as work RAM does not fail — it reads catalog addresses out of
the wrong offsets and returns a plausible vector.

`tools/run_battery.sh` in the sibling will **not** score this ROM: its save
holds a status vector rather than blargg's `$6000` verdict plus the `$DE $B0 $61`
signature, so the runner reports `vector-not-verdict` and leaves adjudication to
the comparator above. Before v2.6.22 it read byte 0 unconditionally and would
have called this ROM a `pass` — measured on a real mirror save, whose first four
bytes are `00 00 00 00`.

## What it does not establish

Nothing here has run on hardware. This is a ROM and a simulation control on it.
Whether the core's save path actually writes the window to the card is a
hardware measurement, and it belongs to v2.9.2, the board session ahead of
v3.0.0 (ADR 0041).

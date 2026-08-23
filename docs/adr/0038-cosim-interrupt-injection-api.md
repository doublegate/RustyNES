# 38. A test-only interrupt-injection API on the emulation core, for rung 2's sweep

Date: 2026-08-23

## Status

Accepted. Amends — narrowly and explicitly — the "the emulation core is
untouched" contract of
[ADR 0037](0037-mister-fpga-core-independent-hdl-implementation.md) and of
`to-dos/plans/v2.5.0-fabric-plan.md`. Does not supersede ADR 0037; that ADR's
provenance firewall, replay-not-lockstep design and rung ladder all stand
unchanged.

## Context

Rung 2 of the Fabric programme is *"6502 cycle-exact bus and interrupts"*, and
its stated completion criterion includes a swept interrupt-injection matrix:
NMI and IRQ asserted at every master-clock offset across roughly twenty hazard
opcodes — branch page-cross, RMW, `BRK`, the NMI/`BRK` hijack, `PLP`/`SEI`/`CLI`
delayed-`I`.

v2.5.0 implemented the DUT half. `cpu6502` has `nmi_n` and `irq_n`, the /NMI
edge latch, the level-sampled /IRQ, the hijack decided at the push rather than at
entry, and the delayed-`I` rule falling out of where the poll sits. The
testbench can assert either pin at a chosen cycle for a chosen duration
(`--nmi-at`, `--nmi-len`, `--irq-at`, `--irq-len`).

**None of it can be compared against anything.** Co-simulation requires the same
stimulus on both sides, and the oracle has no way to receive it. This was
searched rather than assumed: `rustynes-core` exposes no injection entry point,
and the interrupts it does produce come from hardware the CPU rung does not
model — /IRQ from the APU frame counter or a mapper, /NMI from the PPU.

So three of v2.5.0's stated gates are unreachable, and only one of them for a
reason that more RTL could fix:

| gate | state | why |
|---|---|---|
| per-cycle bus equality, opcode groups | met | 4537 cycles, nine ROMs |
| nestest, bounded | met | 27,388 cycles, first `$2002` read is the bound |
| nestest 0-diff, whole run | blocked | needs a PPU — rung 3 |
| bus equality over 5 M cycles | blocked | same wall, sooner |
| interrupt-injection sweep | blocked | **no oracle-side stimulus** |

The last row is what this ADR is about. `BRK` is already oracle-verified and
covers the shared machinery — the seven-cycle sequence, the three pushes, the
vector fetch, `I` set after the push, bit 4 of the pushed `P`. What `BRK` cannot
reach is everything that depends on a *pin*: the edge-versus-level distinction,
an interrupt arriving mid-instruction, the hijack, and delayed-`I`.

Those are precisely where 6502 implementations are known to differ, which is why
the plan put them in rung 2 rather than treating them as incidental.

### Options considered

**A. Do nothing; leave the pin behaviour unverified.** Honest, and the current
state. But it leaves the RTL's most error-prone region resting on a reading of
the documentation, in a programme whose entire premise is that a reading of the
documentation is not evidence. It also means rung 2 can never close, so rung 3
would begin on an unverified foundation.

**B. Wait for the PPU and APU rungs (v2.6+).** Then the oracle's own PPU could
generate an NMI and its APU an IRQ, and the DUT would generate them too. This is
the option that touches nothing. It is rejected as the *primary* route because
it inverts the ladder: rung 3 would have to be built and trusted before rung 2
could be checked, and a divergence would then be ambiguous between the CPU's
interrupt handling and the PPU that produced the interrupt — which is the exact
ambiguity the ladder exists to prevent.

**C. Compare against a third emulator instead.** `scripts/mesen2_cpu_boot_trace.lua`
already writes this project's trace format from a foreign emulator, so the
precedent exists. Rejected because it does not solve the problem: the
third-party emulator needs the same injection capability, and arranging it there
is strictly harder than here, with no way to confirm the two stimuli are
identical.

**D. A test-only injection API on `rustynes-core`.** Chosen, with constraints
that are the substance of this decision rather than a footnote.

## Decision

Add an interrupt-injection API to `rustynes-core`, under the following
constraints. **Each one is a condition of acceptance, not a recommendation.**

1. **Feature-gated and default-off.** It lives behind a new
   `cosim-interrupt-inject` feature. The default build, every shipped binary and
   every existing CI invocation compile as though it does not exist.

2. **Zero hot-path cost when off**, with an executable gate. No branch, no
   field, no state on `Bus::tick_one_cpu_cycle` or any path a user's frame runs
   through. *"Within noise" is not a pass/fail criterion*, so the gate is two
   checks and the first is decisive:

   **2a — structural, and the one that actually settles it.** Every injected
   field and every injected branch is behind `#[cfg(feature =
   "cosim-interrupt-inject")]`, so a default build emits none of it. Verify by
   grepping the expanded source:
   **must be 0** for the default build. This is a proof, not a sample.

   **Do not run it as `cargo expand ... 2>/dev/null | grep -c inject_`.** That
   was this ADR's first wording and it is a trap: `cargo-expand` is a separate
   binary and is not installed on this workstation, so the redirect swallows
   "no such command", `grep -c` counts an empty stream, and the gate reports the
   **0 it is looking for** while measuring nothing. Use the expander that ships
   with the toolchain, and always read the control first:

   ```bash
   # Each expansion is captured and its STATUS checked before anything is
   # counted. Piping cargo straight into `grep -c` hides a failed expansion the
   # same way the `cargo expand` version hid a missing binary: the count comes
   # back 0 and the gate reads that as success. `grep -c` is deliberately NOT
   # used under `set -e` either -- zero matches exits 1, which would abort the
   # very case the gate is looking for.
   expand() {
     local out
     out=$(cargo +nightly rustc -p rustynes-core --lib --profile check "$@" \
             -- -Zunpretty=expanded 2>/dev/null) || return 1
     [ -n "$out" ] || return 1          # an empty expansion is not a clean one
     printf '%s\n' "$out" | grep -c inject_ || true
   }
   off=$(expand) || { echo "default expansion FAILED -- gate not run" >&2; exit 1; }
   on=$(expand --features cosim-interrupt-inject) \
       || { echo "feature expansion FAILED -- gate not run" >&2; exit 1; }
   # off must be 0 AND on must be > 0. A zero `on` means the instrument is dead,
   # not that the feature is clean -- which is exactly how the first run of this
   # gate passed twice while measuring nothing at all.
   [ "$off" -eq 0 ] && [ "$on" -gt 0 ] || { echo "gate FAILED: off=$off on=$on" >&2; exit 1; }
   ```

   Measured 2026-08-23: **off = 0, on = 17.**

   **2b — a calibrated same-tree control, NOT a cross-tree comparison.**

   The first draft of this gate said "against a `main` worktree baseline". That
   instrument was tried and is **invalid**, and the measurement that retired it is
   worth keeping: the default build measured **+3.3%** on `flowing_palette`
   against a `main` worktree whose source, post-`cfg`-expansion, is *provably
   identical* — every added line in `bus.rs` is behind the feature gate, and 2a
   reports 0. Two builds of the same code in different absolute paths differ in
   embedded strings and therefore in layout, and that is what was being measured.

   Reproducible, too: two runs gave +2.9% and +3.3%, so it is not thermal drift.
   A stable number from an instrument that cannot be measuring what it claims is
   worse than a noisy one, because it invites exactly the argument this
   constraint exists to prevent.

   What is valid, **measured on this implementation**:

   | measurement | result |
   |---|---|
   | 2a — expanded-source `inject_` count, default build (command above) | **off = 0, on = 17** — decisive |
   | Same-tree noise floor (baseline vs itself) | **±1.2%** — the calibrated band |
   | Same-tree A/B, feature ON vs OFF | +1.3% / −1.4% / +1.7% / −1.0% — **mixed signs, no consistent direction**; three of the four marginally EXCEED the ±1.2% floor |

   The A/B row previously read "inside the floor". It is not: +1.3, −1.4 and +1.7
   all lie outside ±1.2. The claim was wrong and is corrected rather than the
   band being widened to fit it. What the row actually shows is a signal with no
   consistent direction, which is what a null looks like on an instrument whose
   floor is about this size -- and it is moot either way, because 2a is 0.

   **Pass: 2a is 0.** The rest is calibration, not a gate — because when 2a is 0
   the off build *is* the previous build, and there is no quantity left to
   measure. The ON-vs-OFF row is recorded for the co-simulation crate's own
   information, not as a merge condition.

   Both are *preconditions of merging*, not follow-ups.

3. **Byte-identical default output, verified.** **RustyNES's own** AccuracyCoin
   **141/141 (RAM decoder)** and **RustyNES's own** full-run nestest 0-diff,
   re-run with the feature absent *and* present-but-unused.

   *These are the oracle's results, not the DUT's.* The co-simulated core's
   nestest evidence is **bounded at cycle 27396** by its first `$2002` read, and
   nothing in this ADR extends it — full-run DUT verification is deferred to
   rung 3, when a PPU exists. Spelling that out because the two "nestest 0-diff"
   claims in this programme mean different things and a reader can reasonably
   take the wrong one. A feature that changes behaviour when merely compiled in is the
   `irq-timing-trace` defect — a different per-dot loop selected by a trace flag —
   and that one reached the accuracy battery itself. This is the specific failure
   this constraint exists to prevent, and it is why "present but unused" is a
   distinct case from "absent".

4. **Injection only, observation unchanged.** The API sets the pin state the CPU
   samples. It does not bypass the poll, does not force a vector, and does not
   short-circuit the sequence. If the DUT and the oracle disagree about *when* an
   asserted pin is taken, that disagreement must survive — it is the finding.

5. **The excluded crate is the only consumer.** `rustynes-cosim` enables the
   feature; nothing in the workspace does. The existing
   `cosim_manifest_audit.rs` already asserts that crate stays outside the
   workspace, and that assertion becomes load-bearing here: cargo unifies
   features across a workspace build, so a member enabling this would compile it
   into the accuracy battery.

6. **The amendment is written where the contract is.** ADR 0037's "the emulation
   core is untouched" and the plan's hard-contract section both gain a pointer to
   this ADR. A contract amended in one place and quoted unamended in another is
   how the original claim survives its own revision.

## Consequences

**The sweep becomes runnable, and rung 2 can close on its own terms.** NMI and
IRQ at every offset across the hazard opcodes, compared per cycle on `pc`,
`bus_addr`, `bus_data` and `bus_access` — the four fields already gated.

**The "untouched core" claim is now conditional, and every restatement of it
must say so.** This is the real cost. That claim has been load-bearing in eight
releases of user-facing notes, and its value came from being unqualified. It is
now "untouched in the default build, with one default-off test feature", which is
weaker and must be written that way rather than quietly retained.

**A new way to be wrong exists.** A default-off feature is exactly the shape that
drifts: it is not exercised by the shipped path, so a change that breaks it
surfaces late and somewhere unrelated. Constraint 3's dual re-verification is the
mitigation, and it must run on every release that touches the core, not once at
introduction.

**Two of the three blocked gates remain blocked.** This ADR does not reach
nestest 0-diff or the 5 M-cycle window; both need a PPU and are rung 3. Nothing
here should be read as closing them, and v2.5.0's notes must continue to say so.

**If constraint 2 or 3 fails, the decision is void.** Not "revisit" — void. The
fallback is option B, waiting for rung 3, and accepting that rung 2's pin
behaviour stays unverified until then. Recording the fallback here is deliberate:
a decision whose failure mode is undefined tends to get argued into acceptance
after the fact.

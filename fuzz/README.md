# RustyNES Fuzz Harnesses

`cargo-fuzz` targets for property-style testing of untrusted-input surfaces.

Per `docs/testing-strategy.md` §Layer 5 (Property + Fuzz Testing). Each target
runs under libFuzzer (via the `libfuzzer-sys` crate) and is built/run with
`cargo-fuzz`.

## Setup

```bash
cargo install cargo-fuzz
```

`cargo-fuzz` requires a **nightly** Rust toolchain for the sanitizer flags it
threads through `rustc`. Install with `rustup toolchain install nightly`.

## Targets

| Target | Surface | Why |
|---|---|---|
| `cartridge_parser` | `rustynes_mappers::parse(&[u8])` | iNES / NES 2.0 header is attacker-controlled input |
| `cpu_step` | `Cpu::step(&mut bus)` | 256-opcode dispatch incl. unofficial / JAM opcodes |
| `mapper_writes` | `Mapper::cpu_write` + `ppu_write` + notify_* | Bank-table OOB, IRQ counter overflow |

## Running

```bash
# Single one-shot run (1M iterations or until a finding):
cargo +nightly fuzz run cartridge_parser

# Time-bounded run:
cargo +nightly fuzz run cartridge_parser -- -max_total_time=300

# Build the harness without running (compile-only smoke test):
cargo build --manifest-path fuzz/Cargo.toml
```

## Corpus management

Findings land in `fuzz/artifacts/<target>/`. Interesting reproducers can be
copied into `fuzz/corpus/<target>/` to be picked up by subsequent runs as seed
inputs. The corpus + artifacts directories are gitignored (`.gitignore`
covers `/fuzz/corpus/`, `/fuzz/artifacts/`, `/fuzz/target/`,
`/fuzz/coverage/`).

## CI integration

Not currently wired into CI -- the value of fuzzing is in long-running
campaigns rather than per-commit. The build smoke test
(`cargo build --manifest-path fuzz/Cargo.toml`) verifies the harnesses
compile on every PR via the standard workspace clippy/check jobs.

To run a CI-friendly time-bounded fuzz campaign:

```bash
for target in cartridge_parser cpu_step mapper_writes; do
    cargo +nightly fuzz run "$target" -- -max_total_time=120 -runs=0
done
```

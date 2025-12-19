---
name: Bug Report
about: Create a report to help us improve RustyNES
title: '[BUG] '
labels: bug
assignees: ''
---

## Bug Description

A clear and concise description of the bug.

## Steps to Reproduce

1. Launch RustyNES with '...'
2. Load ROM '...'
3. Perform action '...'
4. Observe error

## Expected Behavior

What you expected to happen.

## Actual Behavior

What actually happened.

## System Information

**RustyNES Version:**

- Version: [e.g., v0.1.0 or git commit hash]
- Build: [Release/Debug]

**Operating System:**

- OS: [e.g., Windows 11, Ubuntu 24.04, macOS 15.0]
- Architecture: [x64, ARM64]

**Hardware:**

- CPU: [e.g., Intel i7-9700K, AMD Ryzen 5 5600X]
- GPU: [e.g., NVIDIA RTX 3060, AMD RX 6700 XT]
- RAM: [e.g., 16 GB]

**ROM Information:**

- Game: [e.g., Super Mario Bros.]
- Format: [iNES, NES 2.0, NSF]
- Mapper: [e.g., 0 (NROM), 1 (MMC1), 4 (MMC3)]
- Region: [NTSC, PAL]

## Log Output

<details>
<summary>Click to expand logs</summary>

```
Paste relevant log output here (enable debug logging with RUST_LOG=debug)
```

</details>

## Screenshots/Videos

If applicable, add screenshots or video recordings to help explain the problem.

## Test ROM Results

If this is an accuracy issue, please run relevant test ROMs and paste results:

```bash
cargo test nestest
cargo test blargg_cpu
cargo test blargg_ppu
```

## Additional Context

Any other context about the problem (e.g., only happens with specific ROMs, timing-dependent, etc.)

## Checklist

- [ ] I searched existing issues to avoid duplicates
- [ ] I tested with the latest main branch
- [ ] I included all relevant system information
- [ ] I provided clear reproduction steps

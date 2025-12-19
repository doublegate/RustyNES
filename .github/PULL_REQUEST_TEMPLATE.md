# Pull Request

## Description

Brief description of what this PR accomplishes.

Fixes #(issue)

## Type of Change

- [ ] Bug fix (non-breaking change that fixes an issue)
- [ ] New feature (non-breaking change that adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] Documentation update
- [ ] Performance improvement
- [ ] Code refactoring
- [ ] Test additions/improvements

## Component(s) Affected

- [ ] CPU (6502/2A03)
- [ ] PPU (2C02)
- [ ] APU (2A03 audio)
- [ ] Mappers
- [ ] Memory/Bus
- [ ] Input handling
- [ ] GUI (Desktop/Web)
- [ ] Save states
- [ ] Netplay
- [ ] TAS tools
- [ ] Lua scripting
- [ ] RetroAchievements
- [ ] Documentation
- [ ] Build system
- [ ] CI/CD

## Changes Made

Detailed list of changes:

- Changed X to Y because Z
- Added A to support B
- Removed C as it was deprecated

## Testing Performed

### Unit Tests

```bash
cargo test --package rustynes-cpu
# Output: All tests passed (X/X)
```

### Integration Tests

- [ ] All existing tests pass
- [ ] New tests added for new functionality
- [ ] Test coverage maintained or improved

### Test ROM Validation

For accuracy-critical changes (CPU/PPU/APU/mappers):

```bash
cargo test nestest          # Golden log validation
cargo test blargg_cpu       # CPU instruction tests
cargo test blargg_ppu       # PPU timing tests
cargo test blargg_apu       # APU tests
```

Results:

- nestest: [PASS/FAIL - details]
- blargg_cpu: [PASS/FAIL - details]
- blargg_ppu: [PASS/FAIL - details]

### Manual Testing

Games tested with this change:

- [ ] Game 1 (Mapper X) - Status
- [ ] Game 2 (Mapper Y) - Status

### Performance Impact

If applicable, benchmark results:

```bash
cargo bench --bench cpu_benchmark
```

- Before: X ops/sec
- After: Y ops/sec
- Change: +/-Z%

## Screenshots/Videos

If applicable, add visual evidence of changes (especially for GUI/rendering changes).

## Documentation

- [ ] Code comments added/updated
- [ ] Rustdoc documentation updated
- [ ] User documentation updated (docs/ folder)
- [ ] CHANGELOG.md updated (for user-facing changes)
- [ ] Examples updated (if API changed)

## Checklist

### Code Quality

- [ ] Code follows Rust style guidelines (cargo fmt)
- [ ] No clippy warnings (cargo clippy -- -D warnings)
- [ ] No new compiler warnings
- [ ] Self-reviewed code for logic errors
- [ ] Complex sections have explanatory comments
- [ ] No unsafe code (unless absolutely necessary and documented)

### Testing

- [ ] All existing tests pass
- [ ] New tests cover new functionality
- [ ] Edge cases are tested
- [ ] Regression tests added for bug fixes

### Documentation

- [ ] Public APIs have rustdoc comments
- [ ] Non-obvious implementation details are commented
- [ ] User-facing changes documented
- [ ] CONTRIBUTING.md reviewed and followed

### Compatibility

- [ ] Changes are backward compatible
- [ ] Save state format unchanged (or migration provided)
- [ ] Configuration format unchanged (or migration provided)
- [ ] Breaking changes clearly documented

### Legal

- [ ] No copyrighted code included
- [ ] All dependencies properly licensed
- [ ] Contributor License Agreement signed (if required)

## Related Issues/PRs

- Related to #(issue)
- Depends on #(PR)
- Blocks #(issue)

## Additional Notes

Any additional information for reviewers (implementation decisions, tradeoffs, future work, etc.)

---

## For Maintainers

- [ ] PR title follows conventional commit format
- [ ] Labels applied correctly
- [ ] Milestone assigned (if applicable)
- [ ] Reviewed for security implications
- [ ] Performance impact assessed

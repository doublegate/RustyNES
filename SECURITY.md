# Security Policy

## Supported Versions

RustyNES is currently in pre-release development. Security updates will be provided for the following versions:

| Version | Supported          |
| ------- | ------------------ |
| main    | :white_check_mark: |
| < 1.0   | :x:                |

Once v1.0.0 is released, this policy will be updated to reflect long-term support commitments.

## Security Considerations for Emulators

While emulators primarily execute untrusted code in a sandboxed environment (the emulated NES hardware), several security considerations apply to RustyNES:

### ROM File Parsing

ROM files (iNES, NES 2.0, NSF, etc.) are untrusted input that must be parsed safely:

- **Buffer overflows**: ROM parsers must validate all size fields and prevent out-of-bounds access
- **Integer overflows**: Calculations involving ROM sizes must be checked
- **Malformed headers**: Invalid or malicious headers should be rejected gracefully

### Save State Deserialization

Save states contain serialized emulator state that could be crafted maliciously:

- **Arbitrary code execution**: Never deserialize Rust code or function pointers
- **Memory corruption**: Validate all pointers and sizes before restoring state
- **Resource exhaustion**: Limit maximum save state sizes

### Network Features (Netplay)

When netplay is implemented (Phase 2), additional attack vectors exist:

- **Denial of service**: Rate limiting and connection validation required
- **State manipulation**: All netplay state must be validated and checksummed
- **Privacy**: Player information and IP addresses must be handled carefully

### Scripting (Lua)

The Lua scripting interface (Phase 2) provides powerful automation but requires sandboxing:

- **Filesystem access**: Restrict or audit all file operations
- **Network access**: Block or require explicit permission for network calls
- **Resource limits**: Enforce CPU time and memory limits for scripts
- **API surface**: Minimize exposed APIs to essential functions only

### External Libraries

RustyNES depends on external crates that may have their own vulnerabilities:

- **FFI bindings**: rcheevos (RetroAchievements) uses unsafe FFI
- **Dependency audits**: Run `cargo audit` regularly
- **WASM security**: WebAssembly builds must not expose local filesystem

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

### How to Report

Send security vulnerability reports to:

**Email**: <parobek@gmail.com>

**Subject**: `[SECURITY] RustyNES - Brief Description`

### What to Include

Please provide the following information:

1. **Description**: Detailed description of the vulnerability
2. **Impact**: What could an attacker accomplish by exploiting this?
3. **Reproduction**: Step-by-step instructions to reproduce the issue
4. **Proof of Concept**: If applicable, a minimal test case or exploit
5. **Affected Versions**: Which versions of RustyNES are affected
6. **Suggested Fix**: If you have ideas for how to fix it (optional)

### Example Report Template

```
Subject: [SECURITY] RustyNES - Buffer Overflow in iNES Header Parser

Description:
A buffer overflow exists in the iNES header parser when processing
oversized PRG ROM size fields.

Impact:
An attacker could craft a malicious .nes file that causes RustyNES
to crash or potentially execute arbitrary code.

Reproduction:
1. Create a .nes file with iNES header byte 4 set to 0xFF
2. Load the file in RustyNES
3. Observe out-of-bounds memory access

Proof of Concept:
[Attached: malicious.nes]

Affected Versions:
- main branch as of commit abc123
- All unreleased versions

Suggested Fix:
Add bounds checking in rom_loader.rs at line 42 before allocating
PRG ROM buffer.
```

### Response Timeline

- **Initial Response**: Within 48 hours (acknowledgment of report)
- **Triage**: Within 5 business days (severity assessment)
- **Status Update**: Weekly until resolved
- **Fix Development**: Depends on severity (see below)
- **Public Disclosure**: After fix is released (coordinated disclosure)

### Severity Levels

| Severity | Response Time | Description |
|----------|---------------|-------------|
| **Critical** | 24-48 hours | Remote code execution, arbitrary file access |
| **High** | 3-7 days | Denial of service, memory corruption |
| **Medium** | 1-2 weeks | Information disclosure, resource exhaustion |
| **Low** | 2-4 weeks | Minor security issues with limited impact |

### What to Expect

1. **Acknowledgment**: We'll confirm receipt of your report
2. **Investigation**: We'll investigate and validate the issue
3. **Fix Development**: We'll develop and test a fix
4. **Disclosure**: We'll coordinate public disclosure with you
5. **Credit**: We'll credit you in the security advisory (unless you prefer to remain anonymous)

### Coordinated Disclosure

We follow responsible disclosure practices:

- **Embargo Period**: Typically 90 days from initial report
- **Early Disclosure**: If actively exploited in the wild, we may release earlier
- **Credit**: Security researchers will be credited in SECURITY-ADVISORIES.md
- **CVE Assignment**: Critical/High severity issues will receive CVE identifiers

### Public Disclosure

After a fix is released, we will:

1. Publish a security advisory on GitHub
2. Update SECURITY-ADVISORIES.md with details
3. Credit the reporter (with permission)
4. Notify users through GitHub Discussions/Releases

### Hall of Fame

Security researchers who have responsibly disclosed vulnerabilities will be credited here:

(No vulnerabilities reported yet)

---

## Security Best Practices for Users

### ROM Sources

Only load ROMs from trusted sources:

- **Homebrew**: Download from official homebrew sites
- **Test ROMs**: Use official test ROM repositories (blargg, nestest, etc.)
- **Commercial ROMs**: Legal backups of games you own

### Save States

Save states should be treated as potentially malicious input:

- **Unknown Sources**: Don't load save states from untrusted sources
- **File Inspection**: Verify file sizes are reasonable (< 10 MB typical)
- **Backups**: Keep backups of important save files

### Netplay (When Available)

When using netplay features:

- **Trusted Players**: Only connect to players you trust
- **Public Servers**: Be cautious with public netplay servers
- **Port Forwarding**: Understand the security implications of opening ports

### Lua Scripts (When Available)

When using Lua scripting:

- **Script Review**: Review Lua scripts before running them
- **Trusted Sources**: Only use scripts from trusted authors
- **Permissions**: Pay attention to filesystem/network access requests

### Updates

Keep RustyNES updated:

- **Latest Version**: Use the latest stable release
- **Security Patches**: Apply security updates promptly
- **Release Notes**: Read release notes for security fixes

---

## Security Audits

RustyNES has not yet undergone a formal security audit. Community security reviews are welcome.

### Planned Security Measures

- **Fuzzing**: Implement fuzz testing for ROM parsers (Phase 2)
- **Static Analysis**: Integrate cargo-audit and cargo-deny in CI
- **WASM Sandboxing**: Ensure WASM builds have no local filesystem access
- **Dependency Scanning**: Automated dependency vulnerability scanning
- **Memory Safety**: Minimize unsafe code, audit all FFI boundaries

### Current Security Posture

- **Language**: Rust provides memory safety by default
- **Unsafe Code**: Minimized to FFI boundaries (rcheevos, platform APIs)
- **Fuzzing**: Not yet implemented
- **Dependency Scanning**: Planned for CI integration
- **Security Testing**: Ad-hoc, not formalized

---

## Contact

- **Security Issues**: <parobek@gmail.com>
- **General Issues**: [GitHub Issues](https://github.com/doublegate/RustyNES/issues)
- **Discussions**: [GitHub Discussions](https://github.com/doublegate/RustyNES/discussions)

---

**Thank you for helping keep RustyNES and its users safe!**

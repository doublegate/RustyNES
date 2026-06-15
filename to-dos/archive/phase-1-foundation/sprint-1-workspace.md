# Sprint 1 — Workspace + CI + lints

**Phase:** Phase 1 — Foundation
**Sprint goal:** A green `cargo build --workspace` + `cargo clippy` + `cargo test` on Linux/macOS/Windows in CI, with all 7 crates present as empty stubs.
**Estimated duration:** 1 week

## Tickets

### T-11-001 — Initialize git repo

**Description:** `git init` in the project root, configure `.gitignore` from `assets/gitignore/rust.gitignore`, ensure `target/` and IDE files are ignored.

**Acceptance criteria:**

- [x] `git status` shows clean tree on a fresh clone.
- [x] `.gitignore` excludes `target/`, `*.rs.bk`, `Cargo.lock` (workspace policy: keep for binaries, ignore for libraries — we keep it since the frontend is a binary).
- [x] Initial commit titled `chore: initialize repository`.

**Dependencies:** none.
**Reference:** `docs/build-and-tooling.md`.
**Estimated complexity:** S.

---

### T-11-002 — Create Cargo workspace manifest

**Description:** Workspace `Cargo.toml` listing all 7 crates as members. Workspace-level `[workspace.package]` (edition, MSRV, license, repository), `[workspace.lints]` (pedantic clippy with allow-list), `[workspace.dependencies]` (centralize versions for shared deps).

**Acceptance criteria:**

- [x] All 7 crates listed in `[workspace] members = [...]`.
- [x] `edition = "2021"`, `rust-version = "1.86"`. (MSRV bumped from 1.75 → 1.86 during bootstrap; see project `CLAUDE.md` for rationale.)
- [x] `license = "MIT OR Apache-2.0"`.
- [x] Lints config matches `docs/build-and-tooling.md` §Linting policy.
- [x] `cargo metadata --format-version 1` succeeds.

**Dependencies:** T-11-001.
**Reference:** `docs/build-and-tooling.md`, `docs/architecture.md` §Workspace shape.
**Estimated complexity:** S.

---

### T-11-003 — Stub all 7 crates with empty `lib.rs` / `main.rs`

**Description:** For each of `rustynes-core`, `rustynes-cpu`, `rustynes-ppu`, `rustynes-apu`, `rustynes-mappers`, `rustynes-frontend`, `rustynes-test-harness`, create `Cargo.toml` and `src/lib.rs` (or `src/main.rs` for the binary). Each crate compiles with a single `pub fn version() -> &'static str { env!("CARGO_PKG_VERSION") }` placeholder.

**Acceptance criteria:**

- [x] `cargo build --workspace` succeeds.
- [x] `cargo doc --workspace --no-deps` succeeds.
- [x] Each crate has its own `Cargo.toml` inheriting from workspace.

**Dependencies:** T-11-002.
**Reference:** `docs/architecture.md` §Workspace shape.
**Estimated complexity:** S.

---

### T-11-004 — Configure `rust-toolchain.toml` and `rustfmt.toml`

**Description:** Pin toolchain to stable 1.86. Set rustfmt config (`imports_granularity = "Crate"`, default everything else). (MSRV bumped from 1.75 → 1.86 during bootstrap; see project `CLAUDE.md` for rationale.)

**Acceptance criteria:**

- [x] `rust-toolchain.toml` pins channel = "1.86".
- [x] `cargo fmt --check` succeeds on a clean repo.
- [x] `rustfmt.toml` documented in `docs/build-and-tooling.md`.

**Dependencies:** T-11-003.
**Reference:** `docs/build-and-tooling.md`.
**Estimated complexity:** S.

---

### T-11-005 — GitHub Actions CI workflow

**Description:** `.github/workflows/ci.yml` that runs on push and PR. Matrix: Linux/macOS/Windows × stable/MSRV. Steps: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo build --workspace`, `cargo test --workspace`, `cargo doc --workspace --no-deps`. Cargo cache via `Swatinem/rust-cache@v2` (the de facto Rust cache action; supersedes hand-rolled `actions/cache` for cargo registry/target).

**Acceptance criteria:**

- [x] Workflow file present and YAML-valid.
- [x] Workflow passes on a test push.
- [x] Cache configured (`Swatinem/rust-cache@v2` covers `~/.cargo/registry` and `target/`).
- [x] fail-fast policy explicitly set (`fail-fast: false`) so each matrix entry reports independently.

**Dependencies:** T-11-004.
**Reference:** `docs/build-and-tooling.md`, `docs/testing-strategy.md` §Layer 6.
**Estimated complexity:** M.

---

### T-11-006 — Issue + PR templates

**Description:** Create `.github/ISSUE_TEMPLATE/bug_report.md`, `.github/ISSUE_TEMPLATE/feature_request.md`, `.github/PULL_REQUEST_TEMPLATE.md` from skill assets.

**Acceptance criteria:**

- [x] All three files present with project-appropriate content.

**Dependencies:** T-11-001.
**Reference:** `docs/build-and-tooling.md`.
**Estimated complexity:** S.

---

### T-11-007 — Root README, LICENSE, CHANGELOG, CONTRIBUTING

**Description:** Populate the standard root files. Dual MIT + Apache-2.0 LICENSE files plus NOTICE.

**Acceptance criteria:**

- [x] `README.md` accurately describes project goals (cycle-accurate NES emulator in Rust).
- [x] `LICENSE-MIT` and `LICENSE-APACHE` present with correct copyright (Parobek, 2026).
- [x] `NOTICE` present.
- [x] `CHANGELOG.md` seeded with `[Unreleased]` section.
- [x] `CONTRIBUTING.md` present with build instructions.

**Dependencies:** T-11-001.
**Reference:** `docs/build-and-tooling.md`.
**Estimated complexity:** S.

---

## Sprint review checklist

When the sprint is closed:

- [x] All tickets above are checked off or explicitly deferred.
- [x] CI pipeline green on the main branch.
- [x] CHANGELOG.md updated for the workspace skeleton.
- [x] `cargo build --workspace` works on a fresh clone with only Rust 1.86 installed.

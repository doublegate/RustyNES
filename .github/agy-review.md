# Review Style Guide (example)

Copy this to **`.github/agy-review.md`** in any repo that uses the Antigravity PR
reviewer (a dedicated filename, so it never collides with an existing `GEMINI.md`
or `AGENTS.md`). Everything here is fed to the reviewer as project-specific rules
to enforce. Delete what does not apply and add your own. Override the path with
the `STYLE_GUIDE` env var in the workflow if you prefer a different location.

## Priorities (in order)

1. Correctness and data integrity.
2. Security: validate all external input at boundaries; no secrets in code, logs,
   or error messages; prefer allowlists.
3. Clear error handling: typed results over panics/`unwrap` on untrusted input.
4. Tests accompany behavior changes.

## Conventions

- Conventional Commits (`feat|fix|docs|refactor|test|chore|perf|build|ci`).
- Match surrounding code style; smallest correct change; reuse existing utilities.
- No emojis in code, comments, commits, or docs.
- Public APIs and non-obvious decisions are documented in the same change.

## What to flag as BLOCKING

- Unvalidated external input reaching a sink (SQL, shell, filesystem, network).
- Hardcoded credentials or tokens.
- Breaking changes to a public API or on-disk/wire format without a version bump.
- Silent failure paths (swallowed errors, ignored return values).

## What to keep as SUGGESTION / NITPICK

- Naming, structure, and readability.
- Missing tests for non-critical paths.
- Performance ideas without a measurement (note: profile before optimizing).

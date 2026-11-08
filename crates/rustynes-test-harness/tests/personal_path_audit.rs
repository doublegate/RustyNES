//! Standing audit — no tracked file names a developer's home directory or an
//! agent harness's per-user scratch tree.
//!
//! **Why this exists.** By v3.6.2 the public repository carried fifty-seven
//! tracked files that spelled out a maintainer's absolute paths:
//! `/home/<name>/Code/...` in one-off scripts and plans, the `~/.claude/...`
//! imports at the top of `AGENTS.md`, agent scratch directories under
//! `/tmp/claude-<uid>/-home-<name>-.../`, and the location of a private ROM
//! library. None of it was a secret, but each line published a username, a
//! private workspace layout or the name of an unrelated private project. Each
//! also made a script silently machine-specific: nine release-automation
//! scripts `cd`-ed into a directory that had not existed since a workspace
//! reorganisation. v3.7.0 scrubbed the current files (maintainer decision,
//! 2026-11-08). This test stops the next one landing, because the paths
//! arrive by copy and paste from a terminal and no reviewer reliably reads
//! for them.
//!
//! **What it flags.** In any tracked, UTF-8 file:
//!
//! - `/home/<name>` and `/Users/<name>`, unless `<name>` is a placeholder
//!   (`user`, `runner`, `you`, ...). `/home/runner` is the GitHub Actions
//!   home and is fine.
//! - `/tmp/claude-<digits>`, the harness scratch root keyed by numeric uid.
//! - `-home-<name>-`, the slug form the harness gives a project's scratch
//!   directory, with the same placeholder rule.
//!
//! Write `~/...`, `$HOME`, a repository-relative path, or `<user>` / `<uid>`
//! instead.
//!
//! **What it deliberately does not cover.**
//!
//! - **Immutable or frozen trees.** `ref-docs/`, `nesdev_wiki/`,
//!   `docs/archive/` and `to-dos/archive/` are never edited in place under this
//!   project's documentation rules. `ref-docs/`'s paths are also provenance
//!   evidence: they record which reference-emulator files a report was written
//!   from.
//! - **Git history.** Paths committed before v3.7.0 stay in old commits. Removing
//!   them would mean rewriting a published history, which the maintainer
//!   declined.
//! - **Email addresses.** The maintainer's contact address is public on purpose
//!   (`SECURITY.md`, `Cargo.toml` authors, every commit's metadata).
//!
//! It walks `git ls-files`, so untracked local state (`salvaged/`, `target/`)
//! is never read. A checkout with no `git` fails the test rather than passing
//! it, because a gate that skips silently is a gate that does not exist.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Workspace root, derived from this crate's manifest dir rather than the CWD.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf()
}

/// Trees this audit does not read, each for the reason in the module docs.
const EXEMPT_PREFIXES: &[&str] = &[
    "ref-docs/",
    "nesdev_wiki/",
    "docs/archive/",
    "to-dos/archive/",
];

/// Names that are placeholders, not anyone's account. Compared case-insensitively.
const PLACEHOLDER_NAMES: &[&str] = &[
    "user", "username", "runner", "you", "me", "name", "someone", "dev", "shared", "ubuntu",
    "builder", "build", "example", "x",
];

const fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')
}

/// Account names that are service accounts the repository itself creates, not
/// a person's, each with where it is defined.
const SERVICE_ACCOUNTS: &[(&str, &str)] = &[(
    "raproxy",
    "the unprivileged user `deploy/Dockerfile.raproxy` creates inside its container",
)];

fn is_placeholder(name: &str) -> bool {
    // `/home/...` is an elision, not a name.
    name.chars().all(|c| c == '.')
        || PLACEHOLDER_NAMES
            .iter()
            .any(|p| p.eq_ignore_ascii_case(name))
        || SERVICE_ACCOUNTS.iter().any(|(s, _)| *s == name)
}

/// Every personal-path finding in `line`, as a short description.
fn findings(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for prefix in ["/home/", "/Users/"] {
        let mut rest = line;
        while let Some(i) = rest.find(prefix) {
            let tail = &rest[i + prefix.len()..];
            let name: String = tail.chars().take_while(|&c| is_name_char(c)).collect();
            if !name.is_empty() && !is_placeholder(&name) {
                out.push(format!("{prefix}{name}"));
            }
            rest = tail;
        }
    }
    let mut rest = line;
    while let Some(i) = rest.find("/tmp/claude-") {
        let tail = &rest[i + "/tmp/claude-".len()..];
        if tail.starts_with(|c: char| c.is_ascii_digit()) {
            let uid: String = tail.chars().take_while(char::is_ascii_digit).collect();
            out.push(format!("/tmp/claude-{uid}"));
        }
        rest = tail;
    }
    let mut rest = line;
    while let Some(i) = rest.find("-home-") {
        let tail = &rest[i + "-home-".len()..];
        // The slug form ends the name at the next '-': `-home-<name>-Code-...`.
        let name: String = tail
            .chars()
            .take_while(|&c| c.is_ascii_alphanumeric() || c == '_')
            .collect();
        if !name.is_empty() && tail[name.len()..].starts_with('-') && !is_placeholder(&name) {
            out.push(format!("-home-{name}-"));
        }
        rest = tail;
    }
    out
}

#[test]
fn no_tracked_file_names_a_home_directory_or_agent_scratch_tree() {
    let root = workspace_root();
    let listed = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-z"])
        .output()
        .expect("`git` must be available: this audit reads the tracked file list");
    assert!(
        listed.status.success(),
        "`git ls-files` failed in {}: not a checkout? {}",
        root.display(),
        String::from_utf8_lossy(&listed.stderr)
    );

    let mut scanned = 0usize;
    let mut bad = Vec::new();
    for rel in listed.stdout.split(|&b| b == 0).filter(|s| !s.is_empty()) {
        let rel = String::from_utf8_lossy(rel);
        if EXEMPT_PREFIXES.iter().any(|p| rel.starts_with(p)) {
            continue;
        }
        let path = root.join(rel.as_ref());
        // Binary and non-UTF-8 files carry no prose; a symlink is read through
        // to its target, which is itself tracked and read on its own turn.
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        scanned += 1;
        for (n, line) in text.lines().enumerate() {
            for f in findings(line) {
                bad.push(format!("{rel}:{}: `{f}`", n + 1));
            }
        }
    }

    // A walk that read nothing would pass vacuously.
    assert!(
        scanned > 1_000,
        "only {scanned} tracked text files were read"
    );
    assert!(
        bad.is_empty(),
        "{} personal path(s) in tracked files; write `~/...`, `$HOME`, a \
         repository-relative path, or `<user>` / `<uid>` instead:\n  {}",
        bad.len(),
        bad.join("\n  ")
    );
}

#[test]
fn the_matcher_flags_real_paths_and_passes_placeholders() {
    // Flagged.
    assert_eq!(findings("cd /home/alice/Code/x"), ["/home/alice"]);
    assert_eq!(findings("open /Users/bob/Library"), ["/Users/bob"]);
    assert_eq!(findings("/tmp/claude-1000/x"), ["/tmp/claude-1000"]);
    assert_eq!(findings("scratch -home-carol-Code-Foo"), ["-home-carol-"]);
    // Two on one line are both reported.
    assert_eq!(findings("/home/a1/x and /home/b2/y").len(), 2);
    // Passed: placeholders, the CI home, and the scrubbed forms.
    for ok in [
        "/home/runner/work/RustyNES",
        "/home/user/.config/rustynes",
        "/Users/you/Library",
        "/tmp/claude-<uid>/-home-<user>-Code",
        "~/Code/OSS_Public-Projects",
        "$HOME/.cargo",
        "a -home- dash with nothing after",
        "a path like `/home/.../x`",
        "WORKDIR /home/raproxy",
    ] {
        assert!(
            findings(ok).is_empty(),
            "{ok:?} flagged: {:?}",
            findings(ok)
        );
    }
}

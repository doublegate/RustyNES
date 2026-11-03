// SPDX-License-Identifier: GPL-3.0-or-later
//! Source checks of the iOS app's persistence (v3.6.0, `T-IOS-DURABLE-WRITE`
//! and `T-IOS-FDS-PERIODIC`, both from #605's review).
//!
//! The Swift in `ios/RustyNES/` cannot be compiled or run in this repository's
//! CI on Linux, and the iOS workflow builds it but runs no tests of the app's
//! own logic. So, like `rustynes-mobile`'s
//! `the_shells_ask_the_bridge_and_parse_nothing_themselves`, this reads the
//! source and pins the shape the two tickets depend on. It proves the code is
//! WIRED as described; it does not prove it compiles or behaves -- that is the
//! device run-sheet's rows T13 and T14 in `docs/mobile-v2.9.3-run-sheet.md`.
//!
//! What it pins:
//!
//! * **One durable writer.** No Swift file other than `DurableWrite.swift`
//!   writes a file with Foundation's `Data.write(to:)` / `String.write(to:)`
//!   (their `.atomic` option renames a scratch file into place but never
//!   `fsync`s it or its directory), and `DurableWrite.swift` itself keeps the
//!   sequence in order: exclusive scratch create, write, `F_FULLFSYNC`,
//!   `rename`, directory `fsync`.
//! * **Every site the ticket names goes through it**: FDS saves, battery saves,
//!   per-game overrides, movies, HD packs, palettes.
//! * **The FDS disk is checked once a second**, from the moment a game opens
//!   until it closes, and the dirty flag is cleared before the image is copied.

use std::fs;
use std::path::{Path, PathBuf};

fn ios_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ios/RustyNES")
}

/// One Swift file, line endings normalised (a Windows checkout has CRLF).
fn swift(name: &str) -> String {
    let p = ios_dir().join(name);
    fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
        .replace("\r\n", "\n")
}

/// Every `*.swift` file under `dir`, recursively, sorted.
fn swift_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            out.extend(swift_files(&path));
        } else if path.extension().is_some_and(|x| x == "swift") {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// The code of a Swift line, without a `//` comment. Good enough for this
/// source (no `//` inside the string literals these checks look at).
fn code(line: &str) -> &str {
    line.find("//").map_or(line, |i| &line[..i])
}

/// The text of `src` from `start` to the first `end` after it.
fn body<'a>(src: &'a str, start: &str, end: &str) -> &'a str {
    let i = src
        .find(start)
        .unwrap_or_else(|| panic!("{start:?} not found"));
    let rest = &src[i..];
    let j = rest
        .find(end)
        .unwrap_or_else(|| panic!("end {end:?} of {start:?} not found"));
    &rest[..j]
}

/// The position of `needle` in `hay`, or a panic naming both.
fn at(hay: &str, needle: &str, what: &str) -> usize {
    hay.find(needle)
        .unwrap_or_else(|| panic!("{what}: {needle:?} not found"))
}

#[test]
fn no_swift_file_writes_a_file_except_through_durable_write() {
    let files = swift_files(&ios_dir());
    assert!(
        files.len() > 40,
        "only {} Swift files found under {}",
        files.len(),
        ios_dir().display()
    );
    // Foundation's file writers: `Data.write(to:options:)`,
    // `String.write(to:atomically:encoding:)`, the `toFile:` variants, and
    // `FileManager.createFile(atPath:contents:)`.
    let banned = [
        ".write(to:",
        ".write(toFile:",
        "options: .atomic",
        "atomically: true, encoding",
        "createFile(atPath:",
    ];
    let mut found = Vec::new();
    for f in &files {
        if f.file_name().is_some_and(|n| n == "DurableWrite.swift") {
            continue;
        }
        let text = fs::read_to_string(f).expect("read swift");
        for (n, line) in text.lines().enumerate() {
            let c = code(line);
            for b in banned {
                if c.contains(b) {
                    found.push(format!("{}:{}: {}", f.display(), n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "file writes outside DurableWrite.swift:\n{}",
        found.join("\n")
    );
}

#[test]
fn durable_write_keeps_the_sequence_in_order() {
    let src = swift("DurableWrite.swift");
    let w = body(
        &src,
        "static func write(_ data: Data, to url: URL) throws {",
        "\n    }\n",
    );
    let create = at(
        w,
        "O_CREAT | O_EXCL | O_NOFOLLOW",
        "exclusive scratch create",
    );
    let scratch = at(
        w,
        "\\(getpid()).\\(counter.next()).tmp",
        "pid + counter scratch name",
    );
    let write = at(w, "try writeAll(fd, data)", "write");
    let full = at(w, "fcntl(fd, F_FULLFSYNC)", "F_FULLFSYNC");
    let fsync = at(w, "fsync(fd) == 0", "fsync fallback");
    let rename = at(w, "rename(from, to)", "rename");
    let dir = at(w, "try syncDirectory(dir)", "directory sync");
    assert!(
        scratch < create,
        "the scratch name is chosen before it is created"
    );
    assert!(
        create < write && write < full && full < fsync && fsync < rename && rename < dir,
        "out of order: create {create}, write {write}, F_FULLFSYNC {full}, fsync {fsync}, rename {rename}, dir {dir}"
    );
    // A failure removes the scratch file and leaves the target alone.
    assert!(w.contains("if !renamed { _ = scratch.path.withCString { unlink($0) } }"));
    // Short writes continue; EINTR retries.
    let all = body(&src, "private static func writeAll(", "\n    }\n");
    assert!(all.contains("if errno == EINTR { continue }"), "{all}");
    assert!(all.contains("left -= n"), "{all}");
    // The directory barrier propagates real errors.
    let d = body(&src, "private static func syncDirectory(", "\n    }\n");
    assert!(d.contains("O_DIRECTORY"), "{d}");
    assert!(
        d.contains("throw Failure(call: \"fsync directory\", code: code)"),
        "{d}"
    );
}

#[test]
fn every_site_the_ticket_names_writes_durably() {
    for (file, what) in [
        ("FdsDiskSaver.swift", "FDS saves"),
        ("BatterySave.swift", "battery saves"),
        ("GameOverrides.swift", "per-game overrides"),
        ("MovieManager.swift", "movies"),
        ("HDPackStore.swift", "HD packs"),
        ("PaletteManager.swift", "palettes"),
    ] {
        let src = swift(file);
        assert!(
            src.lines().any(|l| code(l).contains("DurableWrite.write(")),
            "{what} ({file}) do not write through DurableWrite"
        );
    }
}

#[test]
fn the_fds_disk_is_checked_once_a_second_while_a_game_runs() {
    let app = swift("AppModel.swift");
    // The tick: a repeating 1 s timer that flushes without waiting.
    let poll = body(&app, "private func startFdsPoll() {", "\n    }\n");
    assert!(
        poll.contains("Timer.scheduledTimer(withTimeInterval: 1, repeats: true)"),
        "{poll}"
    );
    assert!(
        poll.contains("self?.flushFdsDiskIfDirty(wait: false)"),
        "{poll}"
    );
    assert!(
        poll.contains("fdsTimer?.invalidate()"),
        "a second game would leave two timers: {poll}"
    );
    // Started once the new game is the current one, stopped when it closes.
    let open = body(
        &app,
        "func openGame(_ entry: LibraryEntry) async {",
        "\n    }\n",
    );
    let current = at(open, "currentEntry = entry", "openGame");
    let start = at(open, "startFdsPoll()", "openGame");
    assert!(
        current < start,
        "the poll starts before the new game is current"
    );
    let close = body(&app, "func closeGame() {", "\n    }\n");
    assert!(close.contains("fdsTimer?.invalidate()"), "{close}");
    // The flush goes through the saver, keyed by the running game.
    let flush = body(
        &app,
        "private func flushFdsDiskIfDirty(wait: Bool = true) {",
        "\n    }\n",
    );
    assert!(
        flush.contains(
            "fdsSaver.flush(core, sha: sha, to: AppModel.fdsSaveUrl(forSha: sha), wait: wait)"
        ),
        "{flush}"
    );

    // The saver: the dirty flag is cleared BEFORE the image is copied, a session
    // ending waits for a queued write first, and a failure is retried.
    let saver = swift("FdsDiskSaver.swift");
    let f = body(&saver, "func flush(_ core: EmulatorCore", "\n    }\n");
    let drain = at(f, "if wait { queue.sync {} }", "FdsDiskSaver.flush");
    let check = at(
        f,
        "core.diskIsDirty || needsRetry(sha)",
        "FdsDiskSaver.flush",
    );
    let clear = at(f, "core.clearDiskDirty()", "FdsDiskSaver.flush");
    let copy = at(f, "core.diskImageBytes()", "FdsDiskSaver.flush");
    assert!(drain < check && check < clear && clear < copy, "{f}");
    assert!(
        f.contains("queue.async(execute: job)"),
        "the tick must not wait: {f}"
    );
    let w = body(&saver, "private func write(_ bytes: Data", "\n    }\n");
    assert!(
        w.contains("if ok { retry.remove(sha) } else { retry.insert(sha) }"),
        "{w}"
    );
}

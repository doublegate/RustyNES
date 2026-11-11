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

// --- v3.7.0 (D24, `T-MOBILE-EXTRAS` rows 1-3, `T-MOBILE-GAME-CORRECTIONS`) ------
//
// The same kind of check for the mobile extras' Swift: it pins that each piece is
// WIRED where the design says, not that it compiles (the iOS CI Simulator build)
// or works (the run sheet's rows W1-W3, E1-E4, X1-X3).

/// A file under `ios/` other than `ios/RustyNES/`, line endings normalised.
fn ios_file(rel: &str) -> String {
    let p = ios_dir().join("..").join(rel);
    fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
        .replace("\r\n", "\n")
}

/// Row 2: the external-display scene gets its own delegate (by role, the iOS 16-26
/// mechanism), the app adopts the delegate that answers, the phone covers its
/// picture while the display is connected, and the display's frames come from the
/// running core and stop with it.
#[test]
fn the_external_display_is_wired_by_role_and_fed_from_the_core() {
    let ext = swift("ExternalDisplay.swift");
    let cfg = body(
        &ext,
        "func application(\n        _ application: UIApplication,\n        configurationForConnecting",
        "\n    }\n",
    );
    assert!(
        cfg.contains("connectingSceneSession.role == .windowExternalDisplayNonInteractive"),
        "{cfg}"
    );
    assert!(
        cfg.contains("config.delegateClass = ExternalDisplaySceneDelegate.self"),
        "{cfg}"
    );
    assert!(
        ext.contains("func sceneDidDisconnect(_ scene: UIScene)")
            && ext.contains("ExternalDisplayHub.shared.disconnected()"),
        "a disconnect must hand the picture back to the phone"
    );
    let app = swift("RustyNESApp.swift");
    assert!(
        app.contains("@UIApplicationDelegateAdaptor(RustyNESAppDelegate.self)"),
        "{app}"
    );
    let game = swift("GameView.swift");
    let metal = at(&game, "MetalGameView(emulator: emulator)", "GameView");
    let card = at(
        &game,
        "ExternalDisplayCard(hub: ExternalDisplayHub.shared)",
        "GameView",
    );
    let pad = at(&game, "MultiTouchControlPad(", "GameView");
    assert!(
        metal < card && card < pad,
        "the card covers the picture (which keeps driving tick()) and the pad stays on top"
    );
    let model = swift("AppModel.swift");
    let open = body(
        &model,
        "func openGame(_ entry: LibraryEntry) async {",
        "\n    }\n",
    );
    assert!(
        open.contains("ExternalDisplayHub.shared.frameSource = { [weak core] in core?.lastFrame }"),
        "{open}"
    );
    let close = body(&model, "func closeGame() {", "\n    }\n");
    assert!(
        close.contains("ExternalDisplayHub.shared.frameSource = nil"),
        "{close}"
    );
}

/// Row 1: the widget target exists and is embedded, both targets share the App
/// Group and the shared file, the widget's link is a scheme the app declares and
/// handles, and opening a game publishes it. Box art is fetched only through the
/// bridge's candidate list, capped, and cached with `DurableWrite` (the banned-writer
/// check above covers every other writer).
#[test]
fn the_widget_and_box_art_are_wired() {
    let yml = ios_file("project.yml");
    assert!(
        yml.contains("  RustyNESWidget:\n    type: app-extension"),
        "widget target"
    );
    assert!(
        yml.contains("      - target: RustyNESWidget\n        embed: true"),
        "the app embeds the widget"
    );
    assert_eq!(
        yml.matches("      - path: Shared\n").count(),
        2,
        "the shared file is in both targets"
    );
    let group = "group.com.doublegate.rustynes";
    assert!(swift("RustyNES.entitlements").contains(group));
    assert!(ios_file("RustyNESWidget/RustyNESWidget.entitlements").contains(group));
    let shared = ios_file("Shared/ResumeWidgetShared.swift");
    assert!(shared.contains(&format!("static let appGroup = \"{group}\"")));
    assert!(shared.contains("URL(string: \"rustynes://resume\")!"));
    let plist = swift("Info.plist");
    assert!(
        plist.contains(
            "<key>CFBundleURLSchemes</key>\n\t\t\t<array>\n\t\t\t\t<string>rustynes</string>"
        ),
        "the app declares the widget's scheme"
    );
    let widget = ios_file("RustyNESWidget/RustyNESWidget.swift");
    assert!(widget.contains(".widgetURL(ResumeWidgetLink.url)"));
    assert!(widget.contains("StaticConfiguration(kind: ResumeWidgetShared.kind"));
    let app = swift("RustyNESApp.swift");
    assert!(app.contains("if ResumeWidgetLink.isResume(url)"), "{app}");
    let model = swift("AppModel.swift");
    let open = body(
        &model,
        "func openGame(_ entry: LibraryEntry) async {",
        "\n    }\n",
    );
    assert!(
        open.contains("ResumeWidgetStore.publish(name: entry.name, sha: entry.sha)"),
        "{open}"
    );

    let art = swift("BoxArt.swift");
    let fetch = body(
        &art,
        "static func fetch(displayName: String, sha: String)",
        "\n    }\n",
    );
    assert!(
        fetch.contains("boxArtCandidateUrls(displayName: displayName)"),
        "{fetch}"
    );
    assert!(fetch.contains("data.count <= maxBytes"), "{fetch}");
    assert!(
        fetch.contains("try DurableWrite.write(data, to: cacheURL(forSha: sha))"),
        "{fetch}"
    );
    assert!(art.contains("static let maxBytes = 2 * 1024 * 1024"));
}

/// Row 3 and the correction switch: the 20-band EQ, the code database and the
/// switch reach the bridge, and a newly opened game gets the player's setting.
#[test]
fn the_eq_cheat_database_and_corrections_reach_the_bridge() {
    let core = swift("EmulatorCore.swift");
    for call in [
        "controller.setEqualizerGains(gainsDb: gains)",
        "controller.cheatDatabaseCodes()",
        "controller.setGameCorrectionsEnabled(enabled: on)",
    ] {
        assert!(core.contains(call), "EmulatorCore does not call {call}");
    }
    let depth = swift("AudioDepth.swift");
    let apply = body(&depth, "func apply(to core: EmulatorCore?) {", "\n    }");
    assert!(
        apply.contains("core?.setEqualizerGains(graphicEqDb)"),
        "{apply}"
    );
    let cheats = swift("CheatsView.swift");
    assert!(cheats.contains("model.emulator?.cheatDatabaseCodes()"));
    let model = swift("AppModel.swift");
    let open = body(
        &model,
        "func openGame(_ entry: LibraryEntry) async {",
        "\n    }\n",
    );
    assert!(
        open.contains("core.setGameCorrectionsEnabled(gameCorrections)"),
        "{open}"
    );
    assert!(model.contains("emulator?.setGameCorrectionsEnabled(gameCorrections)"));
}

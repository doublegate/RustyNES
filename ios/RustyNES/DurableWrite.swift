//
//  DurableWrite.swift
//
//  v3.6.0 (`T-IOS-DURABLE-WRITE`, #605's review): one durable, atomic file
//  write for every path that persists user data -- the Swift counterpart of
//  the desktop's `crates/rustynes-frontend/src/atomic_write.rs`.
//
//  Until v3.6.0 iOS wrote with `Data.write(to:options: .atomic)`. Foundation's
//  `.atomic` writes a scratch file and renames it over the target, which makes
//  the replacement atomic against other readers, but it does not `fsync` the
//  file before the rename or the directory after it. A power loss or a kernel
//  panic between the write and the medium can therefore leave the new directory
//  entry pointing at bytes that never landed: an empty or truncated FDS disk, a
//  battery save, a movie -- precisely the loss the scratch-and-rename exists to
//  prevent. On Apple platforms plain `fsync(2)` is not enough either: it pushes
//  the bytes to the drive but not through the drive's own write cache, and
//  `fcntl(F_FULLFSYNC)` is the call Apple documents for that (fsync(2) man page,
//  "For applications that require tighter guarantees about the integrity of
//  their data, Mac OS X provides the F_FULLFSYNC fcntl").
//
//  The sequence, the desktop module's properties as they apply here:
//
//  1. a SIBLING scratch file, so the rename never crosses a filesystem;
//  2. created EXCLUSIVELY (`O_CREAT | O_EXCL`, `O_NOFOLLOW`), so a planted
//     link or a stale file is never followed or truncated;
//  3. a pid + per-call counter in its name, which is what makes (2) adoptable:
//     a fixed `.tmp` name would fail every save after one crash;
//  4. every byte written (a short `write(2)` is continued, `EINTR` retried);
//  5. `F_FULLFSYNC` on the file (falling back to `fsync` where the filesystem
//     refuses it) BEFORE the rename;
//  6. `rename(2)` over the target, which replaces it atomically;
//  7. `fsync` on the parent directory, so the new entry itself is on the medium;
//  8. on any failure the scratch file is removed and the error propagates --
//     the target is left exactly as it was, and the caller's retry logic (the
//     battery saver's baseline, the FDS retry set) sees a failure, not success.
//
//  Not carried over from the desktop, and why: the target's mode (an app
//  sandbox has no other users, and every file here is the app's own, created
//  0644), and symlink resolution (nothing in the sandbox is a link the user
//  made; the desktop resolves one because users link `config.toml` into a
//  dotfiles repository). The scratch file is created 0644, the mode
//  `Data.write` used.
//
//  The file-protection class: `.atomic` alone (no `.completeFileProtection`
//  option) never set one, so files took the container's default class, and a
//  file created here takes the same default. Nothing changes.
//
//  `crates/rustynes-test-harness/tests/ios_source_audit.rs` fails if any Swift
//  file outside this one writes with `.atomic` / `atomically:` again. This file
//  has never been compiled on the machine that wrote it (Linux); its device
//  check is row T13 of `docs/mobile-v2.9.3-run-sheet.md`.
//

import Darwin
import Foundation

enum DurableWrite {
    /// Why a durable write failed: the system call and its `errno`.
    struct Failure: Error, CustomStringConvertible {
        let call: String
        let code: Int32
        var description: String { "\(call) failed: \(String(cString: strerror(code))) (errno \(code))" }
    }

    /// Per-process counter in the scratch name (property 3).
    private static let counter = Counter()

    /// Write `data` to `url` durably and atomically, creating the parent
    /// directory if it is missing. Throws `Failure` and leaves `url` untouched on
    /// any error.
    static func write(_ data: Data, to url: URL) throws {
        let dir = url.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let scratch = dir.appendingPathComponent(
            ".\(url.lastPathComponent).\(getpid()).\(counter.next()).tmp")

        let fd = scratch.path.withCString { open($0, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o644) }
        guard fd >= 0 else { throw Failure(call: "open", code: errno) }
        var closed = false
        var renamed = false
        defer {
            if !closed { close(fd) }
            if !renamed { _ = scratch.path.withCString { unlink($0) } }
        }

        try writeAll(fd, data)
        // Property 5: through the drive's cache. A filesystem that does not
        // support F_FULLFSYNC (some network and FAT mounts) answers an error;
        // fsync is then the strongest barrier it offers.
        if fcntl(fd, F_FULLFSYNC) != 0 {
            guard fsync(fd) == 0 else { throw Failure(call: "fsync", code: errno) }
        }
        let closeResult = close(fd)
        closed = true
        guard closeResult == 0 else { throw Failure(call: "close", code: errno) }

        let renameResult = scratch.path.withCString { from in
            url.path.withCString { to in rename(from, to) }
        }
        guard renameResult == 0 else { throw Failure(call: "rename", code: errno) }
        renamed = true

        // Property 7. The rename has happened, so a failure here is reported but
        // the new file is in place: the caller retries a write that may already
        // have reached the medium, which is safe.
        try syncDirectory(dir)
    }

    /// Write every byte of `data` to `fd` (property 4).
    private static func writeAll(_ fd: Int32, _ data: Data) throws {
        try data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            guard var p = raw.baseAddress else { return } // empty data: nothing to write
            var left = raw.count
            while left > 0 {
                let n = Darwin.write(fd, p, left)
                if n < 0 {
                    if errno == EINTR { continue }
                    throw Failure(call: "write", code: errno)
                }
                p += n
                left -= n
            }
        }
    }

    /// `fsync` the directory so the entry `rename` created is durable too.
    /// `EINVAL` / `ENOTSUP` mean the filesystem offers no such barrier; failing
    /// the save there would be worse than proceeding (the desktop's rule).
    private static func syncDirectory(_ dir: URL) throws {
        let dfd = dir.path.withCString { open($0, O_RDONLY | O_DIRECTORY | O_CLOEXEC) }
        guard dfd >= 0 else { throw Failure(call: "open directory", code: errno) }
        defer { close(dfd) }
        if fsync(dfd) != 0 {
            let code = errno
            if code == EINVAL || code == ENOTSUP { return }
            throw Failure(call: "fsync directory", code: code)
        }
    }

    /// A thread-safe increasing counter (writes run on several queues).
    private final class Counter: @unchecked Sendable {
        // SAFETY (`@unchecked Sendable`): `value` is touched only under `lock`.
        private let lock = NSLock()
        private var value: UInt64 = 0
        func next() -> UInt64 {
            lock.lock()
            defer { lock.unlock() }
            value &+= 1
            return value
        }
    }
}

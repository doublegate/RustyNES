//
//  SymbolMap.swift
//
//  A host-side symbol-file parser (v1.9.9 "Workshop") for the debugger inspector.
//  RustyNES's core exposes no symbol API (the desktop parser lives in the frontend
//  crate, which the mobile bridge does not depend on), so the iOS debugger parses
//  the common label formats here in Swift and overlays the labels on the
//  bridge-provided disassembly. Read-only; purely a naming aid.
//
//  Supported formats (auto-detected by file extension):
//    .sym  ca65 / WLA-DX        "ADDR LABEL"  (ADDR optionally "bank:addr" / "$addr")
//    .mlb  Mesen label file     "MemoryType:Address:Label[:Comment]"
//    .nl   FCEUX name list      "$ADDR#Name#Comment"
//
//  All parsing is bounds-checked and total: a malformed line is skipped, never a
//  crash, and the label count is capped so a hostile file cannot exhaust memory.
//

import Foundation

/// The symbol-file formats the debugger can load.
enum SymbolFormat {
    case sym, mlb, nl

    /// Resolve a format from a file extension, or nil if unsupported.
    static func from(extension ext: String) -> SymbolFormat? {
        switch ext.lowercased() {
        case "sym": return .sym
        case "mlb": return .mlb
        case "nl": return .nl
        default: return nil
        }
    }
}

/// An address -> label map parsed from a symbol file.
struct SymbolMap {
    /// Hard cap on parsed labels so a hostile / huge file cannot exhaust memory.
    private static let maxLabels = 200_000

    private(set) var labels: [UInt16: String] = [:]

    var count: Int { labels.count }
    var isEmpty: Bool { labels.isEmpty }

    /// The label at `addr`, if any.
    func label(_ addr: UInt16) -> String? { labels[addr] }

    /// Parse `text` in `format` and merge the labels in. Returns the number added.
    @discardableResult
    mutating func merge(_ text: String, format: SymbolFormat) -> Int {
        let before = labels.count
        for rawLine in text.split(whereSeparator: \.isNewline) {
            if labels.count >= Self.maxLabels { break }
            let line = rawLine.trimmingCharacters(in: .whitespaces)
            if line.isEmpty || line.hasPrefix(";") || line.hasPrefix("#") { continue }
            switch format {
            case .sym: parseSym(line)
            case .mlb: parseMlb(line)
            case .nl: parseNl(line)
            }
        }
        return labels.count - before
    }

    // MARK: - Per-format parsers (each total; a bad line is skipped)

    private mutating func parseSym(_ line: String) {
        // "ADDR LABEL" — ADDR may be "$8000", "8000", or "00:8000".
        let parts = line.split(whereSeparator: \.isWhitespace)
        guard parts.count >= 2 else { return }
        guard let addr = Self.parseHexAddr(String(parts[0])) else { return }
        let label = String(parts[1])
        if !label.isEmpty { labels[addr] = label }
    }

    private mutating func parseMlb(_ line: String) {
        // "MemoryType:Address:Label[:Comment]". We only map labelled entries.
        let fields = line.split(separator: ":", omittingEmptySubsequences: false)
        guard fields.count >= 3 else { return }
        guard let addr = Self.parseHexAddr(String(fields[1])) else { return }
        let label = fields[2].trimmingCharacters(in: .whitespaces)
        if !label.isEmpty { labels[addr] = label }
    }

    private mutating func parseNl(_ line: String) {
        // "$ADDR#Name#Comment".
        let fields = line.split(separator: "#", omittingEmptySubsequences: false)
        guard fields.count >= 2 else { return }
        guard let addr = Self.parseHexAddr(String(fields[0])) else { return }
        let name = fields[1].trimmingCharacters(in: .whitespaces)
        if !name.isEmpty { labels[addr] = name }
    }

    /// Parse a hex address token (`$8000`, `8000`, or `bank:8000`) into a `UInt16`.
    /// Returns nil for anything that does not fit (so a 24-bit ROM offset, etc., is
    /// safely skipped rather than truncated into a wrong CPU address).
    private static func parseHexAddr(_ token: String) -> UInt16? {
        var t = token.trimmingCharacters(in: .whitespaces)
        if let colon = t.lastIndex(of: ":") { t = String(t[t.index(after: colon)...]) }
        if t.hasPrefix("$") { t.removeFirst() }
        guard !t.isEmpty, t.count <= 4 else { return nil }
        return UInt16(t, radix: 16)
    }
}

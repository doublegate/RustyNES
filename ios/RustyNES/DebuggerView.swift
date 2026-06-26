//
//  DebuggerView.swift
//
//  A read-only debugger inspector (v1.9.9 "Workshop"): the CPU register file, a
//  disassembly around the program counter, and a CPU-RAM hex view, all snapshotted
//  through the bridge's observational debug API (which never advances or mutates the
//  core). A single "Step frame" advances exactly one frame while the inspector holds
//  the emulator paused. Optional `.sym` / `.mlb` / `.nl` symbol files annotate the
//  disassembly (parsed host-side; see SymbolMap.swift).
//
//  Gated OFF the App-Store build via `BuildChannel`: this developer surface is
//  reachable only on the FOSS / TestFlight channel (ADR 0027 distribution seam).
//  The in-game menu entry and this sheet are both conditional on `BuildChannel.isFoss`.
//

import SwiftUI
import UniformTypeIdentifiers

struct DebuggerView: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss

    @State private var cpu: CpuRegs?
    @State private var disasm: [DisasmRow] = []
    @State private var memBaseHex: String = "0000"
    @State private var memBytes: Data = Data()
    @State private var symbols = SymbolMap()
    @State private var showingSymImporter = false

    private let disasmCount: UInt32 = 32
    private let memWindow: UInt32 = 256

    var body: some View {
        NavigationStack {
            Form {
                cpuSection
                disasmSection
                memorySection
                symbolsSection
            }
            .navigationTitle("Debugger")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button {
                        model.emulator?.debugStep()
                        refresh()
                    } label: {
                        Label("Step", systemImage: "forward.frame")
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .onAppear(perform: refresh)
            .fileImporter(
                isPresented: $showingSymImporter,
                allowedContentTypes: SymbolTypes.importable,
                allowsMultipleSelection: false
            ) { result in
                if case .success(let urls) = result, let url = urls.first {
                    Task { await loadSymbols(from: url) }
                }
            }
        }
    }

    // MARK: - CPU registers

    private var cpuSection: some View {
        Section {
            if let cpu {
                let cols = [GridItem(.flexible()), GridItem(.flexible()), GridItem(.flexible())]
                LazyVGrid(columns: cols, alignment: .leading, spacing: 6) {
                    regCell("A", String(format: "$%02X", cpu.a))
                    regCell("X", String(format: "$%02X", cpu.x))
                    regCell("Y", String(format: "$%02X", cpu.y))
                    regCell("SP", String(format: "$%02X", cpu.s))
                    regCell("PC", String(format: "$%04X", cpu.pc))
                    regCell("P", String(format: "$%02X", cpu.p))
                }
                Text(Self.flagString(cpu.p))
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                LabeledContent("Cycles") {
                    Text("\(cpu.cycles)").font(.caption.monospacedDigit())
                }
                if cpu.jammed {
                    Label("CPU jammed (illegal halt)", systemImage: "exclamationmark.octagon")
                        .font(.caption).foregroundStyle(.red)
                }
            } else {
                Text("No game running.").foregroundStyle(.secondary)
            }
            Button {
                refresh()
            } label: {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
        } header: {
            Text("CPU")
        } footer: {
            Text("Read-only. \u{201C}Step\u{201D} advances exactly one frame while paused.")
        }
    }

    private func regCell(_ name: String, _ value: String) -> some View {
        HStack(spacing: 4) {
            Text(name).font(.caption.bold()).foregroundStyle(.secondary)
            Text(value).font(.body.monospaced())
        }
        .accessibilityElement(children: .combine)
    }

    /// Decode the P register into an `NV-BDIZC` string (uppercase = set).
    private static func flagString(_ p: UInt8) -> String {
        let names: [(UInt8, Character)] = [
            (0x80, "N"), (0x40, "V"), (0x20, "-"), (0x10, "B"),
            (0x08, "D"), (0x04, "I"), (0x02, "Z"), (0x01, "C"),
        ]
        return names.map { bit, ch in
            (p & bit) != 0 ? String(ch) : String(ch).lowercased()
        }.joined(separator: " ")
    }

    // MARK: - Disassembly

    private var disasmSection: some View {
        Section {
            ForEach(disasm, id: \.addr) { row in
                disasmRow(row)
            }
        } header: {
            Text("Disassembly")
        }
    }

    private func disasmRow(_ row: DisasmRow) -> some View {
        let isPC = cpu?.pc == row.addr
        let hex = row.bytes.map { String(format: "%02X", $0) }.joined(separator: " ")
        return HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: isPC ? "arrowtriangle.right.fill" : "arrowtriangle.right")
                .font(.caption2)
                .foregroundStyle(isPC ? Color.accentColor : Color.clear)
            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 8) {
                    Text(String(format: "$%04X", row.addr))
                        .font(.caption.monospaced()).foregroundStyle(.secondary)
                    Text("\(row.mnemonic) \(row.operand)")
                        .font(.body.monospaced())
                }
                if let label = symbols.label(row.addr) {
                    Text(label).font(.caption2.monospaced()).foregroundStyle(.tint)
                }
                Text(hex).font(.caption2.monospaced()).foregroundStyle(.secondary.opacity(0.7))
            }
        }
        .listRowBackground(isPC ? Color.accentColor.opacity(0.12) : nil)
    }

    // MARK: - Memory

    private var memorySection: some View {
        Section {
            HStack {
                Text("Base $")
                TextField("0000", text: $memBaseHex)
                    .font(.body.monospaced())
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.characters)
                    .accessibilityLabel(Text("Base address"))
                    .onSubmit(refresh)
                Spacer()
                Button("Read", action: refresh)
            }
            ForEach(memoryRows(), id: \.addr) { row in
                Text(row.text)
                    .font(.caption2.monospaced())
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        } header: {
            Text("Memory")
        } footer: {
            Text(String(
                format: String(localized: "%d bytes from the base address, side-effect-free."),
                Int(memWindow)
            ))
        }
    }

    private struct MemRow: Identifiable {
        let addr: UInt16
        let text: String
        var id: UInt16 { addr }
    }

    private func memoryRows() -> [MemRow] {
        guard let base = UInt16(memBaseHex.trimmingCharacters(in: .whitespaces), radix: 16) else {
            return []
        }
        var rows: [MemRow] = []
        let bytes = [UInt8](memBytes)
        var offset = 0
        while offset < bytes.count {
            let rowAddr = base &+ UInt16(offset & 0xFFFF)
            let slice = bytes[offset..<min(offset + 16, bytes.count)]
            let hex = slice.map { String(format: "%02X", $0) }.joined(separator: " ")
            let ascii = slice.map { (0x20...0x7E).contains($0) ? Character(UnicodeScalar($0)) : "." }
            let row = String(format: "%04X  %@  %@", rowAddr, hex, String(ascii))
            rows.append(MemRow(addr: rowAddr, text: row))
            offset += 16
        }
        return rows
    }

    // MARK: - Symbols

    private var symbolsSection: some View {
        Section {
            Button {
                showingSymImporter = true
            } label: {
                Label("Load symbol file (.sym / .mlb / .nl)", systemImage: "tag")
            }
            if !symbols.isEmpty {
                LabeledContent("Loaded labels", value: "\(symbols.count)")
                Button(role: .destructive) {
                    symbols = SymbolMap()
                } label: {
                    Label("Clear symbols", systemImage: "trash")
                }
            }
        } header: {
            Text("Symbols")
        } footer: {
            Text("Annotates the disassembly with labels. Parsed on-device; read-only.")
        }
    }

    private func loadSymbols(from url: URL) async {
        guard let format = SymbolFormat.from(extension: url.pathExtension) else { return }
        // The (possibly large) symbol-file read runs off the main actor; the
        // parse + state update hop back.
        let text = await Task.detached(priority: .userInitiated) { () -> String? in
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            guard let data = try? Data(contentsOf: url) else { return nil }
            return String(data: data, encoding: .utf8)
        }.value
        guard let text else { return }
        var map = symbols
        map.merge(text, format: format)
        symbols = map
        refresh()
    }

    // MARK: - Refresh

    private func refresh() {
        guard let emulator = model.emulator else {
            cpu = nil
            disasm = []
            memBytes = Data()
            return
        }
        let regs = emulator.debugCpuState()
        cpu = regs
        disasm = emulator.debugDisassemble(pc: regs.pc, count: disasmCount)
        if let base = UInt16(memBaseHex.trimmingCharacters(in: .whitespaces), radix: 16) {
            memBytes = emulator.debugReadMemory(start: base, len: memWindow)
        }
    }
}

/// The symbol-file UTTypes for import. `.sym` / `.mlb` / `.nl` are plain text; resolve
/// each by extension so the picker shows them, falling back to plain text / data.
enum SymbolTypes {
    static var importable: [UTType] {
        let exts = ["sym", "mlb", "nl"]
        let types = exts.compactMap { UTType(filenameExtension: $0) }
        return types.isEmpty ? [.plainText, .data] : types + [.plainText]
    }
}

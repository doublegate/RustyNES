//
//  CheatsView.swift
//
//  The in-game Cheats panel (v1.9.9 "Workshop"): add / remove Game Genie codes
//  (applied live by the core's own cheat engine, exactly like the desktop), plus a
//  raw-RAM editor that pokes / peeks a CPU-RAM byte via the core's existing
//  poke_ram / peek paths. Reached from the in-game pill menu. Off by default —
//  with no codes added the emulation is byte-identical.
//

import SwiftUI

struct CheatsView: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss

    @State private var codes: [GenieCodeInfo] = []
    @State private var newCode: String = ""
    @State private var addError: String?

    // Raw-RAM editor state (hex text fields).
    @State private var ramAddrHex: String = "0000"
    @State private var ramValueHex: String = "00"
    @State private var ramPeek: UInt8?

    var body: some View {
        NavigationStack {
            Form {
                genieSection
                rawRamSection
            }
            .navigationTitle("Cheats")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .onAppear(perform: refresh)
        }
    }

    // MARK: - Game Genie

    private var genieSection: some View {
        Section {
            HStack {
                TextField("Code (e.g. GOSSIP)", text: $newCode)
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.characters)
                    .onSubmit(addCode)
                Button("Add", action: addCode)
                    .disabled(newCode.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            if let addError {
                Label(addError, systemImage: "exclamationmark.triangle")
                    .font(.caption)
                    .foregroundStyle(.orange)
            }
            if codes.isEmpty {
                Text("No active codes.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            } else {
                ForEach(codes, id: \.code) { code in
                    codeRow(code)
                }
                Button(role: .destructive) {
                    model.emulator?.cheatClearGenie()
                    refresh()
                } label: {
                    Label("Clear all codes", systemImage: "trash")
                }
            }
        } header: {
            Text("Game Genie")
        } footer: {
            Text("6- or 8-character codes are applied live to PRG reads. Enter codes for the game you own.")
        }
    }

    private func codeRow(_ code: GenieCodeInfo) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(code.code)
                    .font(.body.monospaced())
                Text(String(format: "$%04X = $%02X", code.addr, code.data))
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
            }
            Spacer()
        }
        .swipeActions {
            Button(role: .destructive) {
                model.emulator?.cheatRemoveGenie(code.code)
                refresh()
            } label: {
                Label("Delete", systemImage: "trash")
            }
        }
    }

    private func addCode() {
        let code = newCode.trimmingCharacters(in: .whitespaces).uppercased()
        guard !code.isEmpty else { return }
        do {
            try model.emulator?.cheatAddGenie(code)
            newCode = ""
            addError = nil
            refresh()
        } catch {
            addError = error.localizedDescription
        }
    }

    // MARK: - Raw-RAM editor

    private var rawRamSection: some View {
        Section {
            HStack {
                Text("Address")
                Spacer()
                Text("$").foregroundStyle(.secondary)
                TextField("0000", text: $ramAddrHex)
                    .font(.body.monospaced())
                    .multilineTextAlignment(.trailing)
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.characters)
                    .frame(width: 80)
                    .accessibilityLabel(Text("Address"))
                    .onChange(of: ramAddrHex) { _ in ramPeek = nil }
            }
            HStack {
                Text("Value")
                Spacer()
                Text("$").foregroundStyle(.secondary)
                TextField("00", text: $ramValueHex)
                    .font(.body.monospaced())
                    .multilineTextAlignment(.trailing)
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.characters)
                    .frame(width: 48)
                    .accessibilityLabel(Text("Value"))
            }
            HStack {
                Button("Peek", action: peek)
                Spacer()
                Button("Poke", action: poke)
                    .buttonStyle(.borderedProminent)
            }
            .disabled(parsedAddr == nil)
            if let ramPeek {
                LabeledContent("Current") {
                    Text(String(format: "$%02X (%d)", ramPeek, ramPeek))
                        .font(.body.monospaced())
                }
            }
        } header: {
            Text("Memory editor")
        } footer: {
            Text("Read or write one byte of CPU RAM ($0000-$1FFF). A poke is one-shot \u{2014} the game may overwrite it next frame.")
        }
    }

    /// The parsed CPU-RAM address, or nil if it does not parse or is outside the
    /// addressable CPU-RAM window the bridge accepts. The bridge clamps writes to
    /// `$0000-$1FFF` (2 KiB RAM, mirrored), so an out-of-range value would silently
    /// alias into the mirror; reject it here so Peek/Poke stay disabled instead.
    private var parsedAddr: UInt16? {
        guard let addr = UInt16(ramAddrHex.trimmingCharacters(in: .whitespaces), radix: 16),
              addr <= 0x1FFF else { return nil }
        return addr
    }
    private var parsedValue: UInt8? { UInt8(ramValueHex.trimmingCharacters(in: .whitespaces), radix: 16) }

    private func peek() {
        guard let addr = parsedAddr else { return }
        ramPeek = model.emulator?.peekByte(addr: addr)
    }

    private func poke() {
        guard let addr = parsedAddr, let value = parsedValue else { return }
        model.emulator?.pokeRam(addr: addr, value: value)
        ramPeek = model.emulator?.peekByte(addr: addr)
    }

    private func refresh() {
        codes = model.emulator?.cheatGenieCodes() ?? []
    }
}

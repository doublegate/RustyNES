//
//  LuaConsoleView.swift
//
//  A developer affordance (v1.9.6): a Lua console for the running game. Enter / edit a
//  script, Load it into the sandboxed engine (the same one the desktop / Android use),
//  watch its `print` / `emu.log` output, and Unload. The script's `on_frame` runs each
//  frame after the tick, so this sheet does NOT pause emulation (it is presented like
//  the Movies panel). The last script text is persisted (UserDefaults) for convenience.
//
//  Power feature: scripts can read/write emulator memory (gated like the desktop). Kept
//  under a "Developer" entry point rather than the main UI.
//

import SwiftUI

struct LuaConsoleView: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss

    @State private var script: String = ""
    @State private var log: [String] = []
    @State private var isLoaded = false
    @State private var errorMessage: String?

    // Poll the script log a few times a second while the console is open.
    private let logTimer = Timer.publish(every: 0.25, on: .main, in: .common).autoconnect()

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 0) {
                statusBar

                Text("Script")
                    .font(.caption.bold())
                    .foregroundStyle(.secondary)
                    .padding(.horizontal)
                    .padding(.top, 8)

                TextEditor(text: $script)
                    .font(.system(.body, design: .monospaced))
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
                    .frame(minHeight: 160)
                    .overlay(
                        RoundedRectangle(cornerRadius: 8)
                            .strokeBorder(Color.secondary.opacity(0.3))
                    )
                    .padding(.horizontal)

                HStack {
                    Button {
                        load()
                    } label: {
                        Label("Load", systemImage: "play.fill")
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(script.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)

                    Button(role: .destructive) {
                        model.unloadLuaScript()
                        isLoaded = false
                    } label: {
                        Label("Unload", systemImage: "stop.fill")
                    }
                    .buttonStyle(.bordered)
                    .disabled(!isLoaded)

                    Spacer()

                    Button("Clear log") { log = [] }
                        .font(.caption)
                }
                .padding()

                Divider()

                Text("Output")
                    .font(.caption.bold())
                    .foregroundStyle(.secondary)
                    .padding(.horizontal)
                    .padding(.top, 8)

                ScrollView {
                    Text(log.isEmpty ? "No output yet." : log.joined(separator: "\n"))
                        .font(.system(.caption, design: .monospaced))
                        .foregroundStyle(log.isEmpty ? .secondary : .primary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .textSelection(.enabled)
                        .padding(.horizontal)
                }
            }
            .navigationTitle("Lua Console")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .onAppear {
                script = model.lastLuaScript
                isLoaded = model.luaIsLoaded
            }
            .onReceive(logTimer) { _ in
                let lines = model.drainLuaLog()
                if !lines.isEmpty {
                    // Keep a bounded tail so a chatty script can't grow the view unbounded.
                    log = (log + lines).suffix(500).map { $0 }
                }
                isLoaded = model.luaIsLoaded
            }
            .alert(
                "Lua",
                isPresented: Binding(get: { errorMessage != nil }, set: { if !$0 { errorMessage = nil } }),
                actions: { Button("OK", role: .cancel) {} },
                message: { Text(errorMessage ?? "") }
            )
        }
    }

    private var statusBar: some View {
        HStack {
            Circle()
                .fill(isLoaded ? Color.green : Color.secondary)
                .frame(width: 10, height: 10)
            Text(isLoaded ? "Script loaded" : "No script loaded")
                .font(.subheadline)
            Spacer()
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(Color.secondary.opacity(0.08))
    }

    private func load() {
        do {
            try model.loadLuaScript(script)
            isLoaded = true
        } catch {
            errorMessage = error.localizedDescription
            isLoaded = model.luaIsLoaded
        }
    }
}

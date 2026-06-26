//
//  NetplayView.swift
//
//  The in-game netplay panel (v1.9.6): host a direct-IP / LAN session (sharing this
//  device's Wi-Fi IP + the bound port) or join a peer at "ip:port", with a live status
//  readout (phase, ping, desync) and a Leave button. Reached from the in-game pill
//  menu. Like the Movies panel it does NOT pause emulation -- the netplay loop
//  (`npAdvanceFrame`) must keep running while connecting and in-game.
//
//  Scope: direct-IP / same-LAN only this release. Room-code / CGNAT / TURN netplay is
//  v1.9.7. Cross-play with desktop / Android peers on the same LAN is valid (same core
//  + input protocol).
//

import SwiftUI

struct NetplayView: View {
    @ObservedObject private var netplay: NetplayModel
    @Environment(\.dismiss) private var dismiss

    @State private var joinAddress = ""

    init(netplay: NetplayModel) {
        self._netplay = ObservedObject(wrappedValue: netplay)
    }

    var body: some View {
        NavigationStack {
            Form {
                if netplay.isActive {
                    statusSection
                    Section {
                        Button("Leave session", role: .destructive) { netplay.leave() }
                    }
                } else {
                    hostSection
                    joinSection
                }

                Section {
                    Text("Direct-IP play works on the same Wi-Fi / LAN. Both players must load the same ROM. Internet (room-code) play arrives in a later update.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Netplay")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .alert(
                "Netplay",
                isPresented: Binding(get: { netplay.lastError != nil }, set: { if !$0 { netplay.lastError = nil } }),
                actions: { Button("OK", role: .cancel) {} },
                message: { Text(netplay.lastError ?? "") }
            )
        }
    }

    // MARK: - Host

    private var hostSection: some View {
        Section {
            Button {
                netplay.host()
            } label: {
                Label("Host a session", systemImage: "wifi")
            }
            if let port = netplay.hostedPort {
                LabeledContent("Your address",
                               value: "\(netplay.localIPv4 ?? "<this device's IP>"):\(port)")
                    .textSelection(.enabled)
                Text("Share this address with the other player, then have them Join.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        } header: {
            Text("Host")
        } footer: {
            Text("You play as Player 1. The session starts once the other player joins.")
        }
    }

    // MARK: - Join

    private var joinSection: some View {
        Section {
            TextField("host ip:port (e.g. 192.168.1.50:7000)", text: $joinAddress)
                .keyboardType(.numbersAndPunctuation)
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)
            Button {
                netplay.join(address: joinAddress)
            } label: {
                Label("Join session", systemImage: "person.2.fill")
            }
            .disabled(joinAddress.trimmingCharacters(in: .whitespaces).isEmpty)
        } header: {
            Text("Join")
        } footer: {
            Text("Enter the host's address. You play as Player 2.")
        }
    }

    // MARK: - Status

    @ViewBuilder
    private var statusSection: some View {
        Section {
            if let status = netplay.status {
                LabeledContent("State", value: phaseLabel(status.phase))
                LabeledContent("Role", value: status.isHost ? "Host (P1)" : "Joiner (P2)")
                if let ping = status.pingMs {
                    LabeledContent("Ping", value: "\(ping) ms")
                }
                if status.desync {
                    Label("Desync detected", systemImage: "exclamationmark.triangle.fill")
                        .foregroundStyle(.red)
                } else if status.stalled {
                    Label("Re-syncing...", systemImage: "clock.arrow.circlepath")
                        .foregroundStyle(.orange)
                }
                if !status.message.isEmpty {
                    Text(status.message)
                        .font(.caption)
                        .foregroundStyle(status.desync ? .red : .secondary)
                }
            } else {
                ProgressView()
            }
        } header: {
            Text("Session")
        }
    }

    private func phaseLabel(_ phase: NpPhase) -> String {
        switch phase {
        case .idle: return "Idle"
        case .negotiating: return "Negotiating"
        case .connecting: return "Connecting"
        case .inGame: return "Connected"
        case .error: return "Error"
        }
    }
}

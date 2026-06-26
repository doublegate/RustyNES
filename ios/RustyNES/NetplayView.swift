//
//  NetplayView.swift
//
//  The in-game netplay panel (v1.9.6): host a direct-IP / LAN session (sharing this
//  device's Wi-Fi IP + the bound port) or join a peer at "ip:port", with a live status
//  readout (phase, ping, desync) and a Leave button. Reached from the in-game pill
//  menu. Like the Movies panel it does NOT pause emulation -- the netplay loop
//  (`npAdvanceFrame`) must keep running while connecting and in-game.
//
//  v1.9.7 adds a ROOM-CODE (CGNAT / internet) mode alongside direct-IP / LAN: host
//  publishes a shareable room code, join enters one. Both reuse the same in-game
//  `npAdvanceFrame` loop; the relay / STUN / TURN endpoints come from Settings >
//  Netplay. Cross-play with desktop / Android peers is valid (same core + protocol).
//

import SwiftUI

struct NetplayView: View {
    @ObservedObject private var netplay: NetplayModel
    @Environment(\.dismiss) private var dismiss

    /// The connection mode shown while idle (direct-IP / LAN vs. room code).
    private enum Mode: String, CaseIterable, Identifiable {
        case lan, room
        var id: String { rawValue }
        var label: String { self == .lan ? "Same Wi-Fi" : "Room code" }
    }
    @State private var mode: Mode = .lan
    @State private var joinAddress = ""
    @State private var joinRoomCode = ""

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
                    Section {
                        Picker("Mode", selection: $mode) {
                            ForEach(Mode.allCases) { Text($0.label).tag($0) }
                        }
                        .pickerStyle(.segmented)
                    }
                    if mode == .lan {
                        hostSection
                        joinSection
                    } else {
                        roomHostSection
                        roomJoinSection
                    }
                }

                Section {
                    Text(footerText)
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

    // MARK: - Room code (CGNAT / internet, v1.9.7)

    private var roomHostSection: some View {
        Section {
            Button {
                netplay.hostRoom()
            } label: {
                Label("Host a room", systemImage: "globe")
            }
            .disabled(!netplay.signalingConfigured)
            if let code = netplay.hostedRoomCode {
                LabeledContent("Room code", value: code)
                    .textSelection(.enabled)
                    .font(.body.monospaced())
                // ShareLink handles the iPad popover anchoring itself (no manual
                // UIActivityViewController popover config needed).
                ShareLink(item: shareMessage(code: code)) {
                    Label("Share room code", systemImage: "square.and.arrow.up")
                }
                Text("Share this code with the other player, then have them Join.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        } header: {
            Text("Host")
        } footer: {
            Text(netplay.signalingConfigured
                ? "You play as Player 1. Works across different networks via the relay."
                : "Set a signaling relay URL in Settings > Netplay to use room codes.")
        }
    }

    private var roomJoinSection: some View {
        Section {
            TextField("room code", text: $joinRoomCode)
                .autocorrectionDisabled()
                .textInputAutocapitalization(.characters)
                .font(.body.monospaced())
            Button {
                netplay.joinRoom(code: joinRoomCode)
            } label: {
                Label("Join room", systemImage: "person.2.fill")
            }
            .disabled(!netplay.signalingConfigured
                || joinRoomCode.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        } header: {
            Text("Join")
        } footer: {
            Text("Enter the host's room code. You play as Player 2.")
        }
    }

    private func shareMessage(code: String) -> String {
        "Join my RustyNES netplay session with room code: \(code)"
    }

    private var footerText: String {
        mode == .lan
            ? "Direct-IP play works on the same Wi-Fi / LAN. Both players must load the same ROM."
            : "Room-code play connects across networks through a signaling relay (and a TURN server for strict NATs) the maintainer hosts. Both players must load the same ROM."
    }

    // MARK: - Status

    @ViewBuilder
    private var statusSection: some View {
        Section {
            if let status = netplay.status {
                LabeledContent("State", value: phaseLabel(status.phase))
                // The Negotiating sub-step (registering / discovering / punching /
                // relaying) while NAT traversal runs on the room-code path.
                if status.phase == .negotiating, !status.detail.isEmpty {
                    LabeledContent("Step", value: status.detail)
                }
                LabeledContent("Role", value: status.isHost ? "Host (P1)" : "Joiner (P2)")
                if status.relayed {
                    Label("Connected via relay (TURN)", systemImage: "arrow.triangle.branch")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
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

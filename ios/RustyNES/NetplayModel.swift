//
//  NetplayModel.swift
//
//  Direct-IP / same-LAN rollback-netplay session state (v1.9.6). Host binds a local
//  UDP port and shows this device's LAN IP + port to share; Join dials a peer's
//  "ip:port". The frame loop integration lives in `EmulatorCore.tickNetplay`: once a
//  session is active the loop advances via `npAdvanceFrame` instead of `runFrame`, so
//  this model only starts/stops sessions and polls `npStatus()` for the HUD.
//
//  Determinism / cross-play: the same rollback core + input protocol as desktop and
//  Android, so an iOS peer can play a desktop / Android peer on the same LAN.
//
//  Scope: direct-IP / LAN ONLY this release. CGNAT / room-code / STUN / TURN netplay
//  (the bridge's `npHostRoom` / `npJoinRoom`) is deferred to v1.9.7 and not wired here.
//

import Combine
import Darwin
import Foundation

@MainActor
final class NetplayModel: ObservableObject {
    /// The latest status snapshot (nil before a session starts).
    @Published private(set) var status: NpStatus?
    /// The bound local port to share with the joiner (host only).
    @Published private(set) var hostedPort: UInt16?
    /// A transient error surfaced to the netplay UI.
    @Published var lastError: String?

    private weak var core: EmulatorCore?
    private var pollTimer: Timer?

    /// Whether a session is live / connecting (the loop drives via `npAdvanceFrame`).
    var isActive: Bool { core?.npIsActive() ?? false }

    /// This device's Wi-Fi IPv4 address (to show the host), or nil if unavailable.
    var localIPv4: String? { NetworkInfo.wifiIPv4() }

    // MARK: - Lifecycle (driven by AppModel.openGame / closeGame)

    func attach(core: EmulatorCore) { self.core = core }

    func detach() {
        stopPolling()
        core = nil
        status = nil
        hostedPort = nil
    }

    // MARK: - Host / Join / Leave

    /// Host a 2-player session. `port == 0` lets the OS pick (the bound port is then
    /// published in `hostedPort`).
    func host(port: UInt16 = 0) {
        guard let core else { lastError = "Open a game first to host netplay."; return }
        do {
            hostedPort = try core.npHost(localPort: port, numPlayers: 2)
            lastError = nil
            startPolling()
        } catch {
            lastError = "Could not host: \(error.localizedDescription)"
        }
    }

    /// Join a session at `address` ("ip:port").
    func join(address: String) {
        guard let core else { lastError = "Open a game first to join netplay."; return }
        let trimmed = address.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { lastError = "Enter the host's ip:port."; return }
        do {
            try core.npJoin(address: trimmed)
            hostedPort = nil
            lastError = nil
            startPolling()
        } catch {
            lastError = "Could not join: \(error.localizedDescription)"
        }
    }

    /// Leave the session and return to single-player.
    func leave() {
        core?.npLeave()
        hostedPort = nil
        status = nil
        stopPolling()
    }

    // MARK: - Polling

    private func startPolling() {
        guard pollTimer == nil else { return }
        let timer = Timer(timeInterval: 0.25, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.poll() }
        }
        RunLoop.main.add(timer, forMode: .common)
        pollTimer = timer
    }

    private func stopPolling() {
        pollTimer?.invalidate()
        pollTimer = nil
    }

    private func poll() {
        guard let core else { stopPolling(); return }
        guard core.npIsActive() else {
            // The session ended (left elsewhere); reflect it and stop.
            status = nil
            stopPolling()
            return
        }
        status = core.npStatus()
    }
}

/// Resolves the device's local Wi-Fi IPv4 address (for showing the host's "share
/// this address" line). Reads the `en0` interface via `getifaddrs`.
enum NetworkInfo {
    static func wifiIPv4() -> String? {
        var address: String?
        var ifaddr: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&ifaddr) == 0, let first = ifaddr else { return nil }
        defer { freeifaddrs(ifaddr) }

        var ptr: UnsafeMutablePointer<ifaddrs>? = first
        while let current = ptr {
            let interface = current.pointee
            let family = interface.ifa_addr.pointee.sa_family
            if family == UInt8(AF_INET) {
                let name = String(cString: interface.ifa_name)
                // `en0` is the Wi-Fi interface on iPhone/iPad; LAN netplay rides Wi-Fi.
                if name == "en0" {
                    var host = [CChar](repeating: 0, count: Int(NI_MAXHOST))
                    if getnameinfo(
                        interface.ifa_addr,
                        socklen_t(interface.ifa_addr.pointee.sa_len),
                        &host, socklen_t(host.count),
                        nil, 0, NI_NUMERICHOST
                    ) == 0 {
                        address = String(cString: host)
                    }
                }
            }
            ptr = interface.ifa_next
        }
        return address
    }
}

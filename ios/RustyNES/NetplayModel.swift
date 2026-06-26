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
//  v1.9.7 adds a ROOM-CODE (CGNAT / internet) path alongside direct-IP: `hostRoom`
//  registers with a signaling relay and publishes a room code to share; `joinRoom`
//  dials that code. Both build an `NpNetConfig` from the persisted netplay settings
//  (a required `wss://` signaling URL, optional STUN list, optional TURN trio) and
//  then run the SAME `npAdvanceFrame` loop as the LAN path -- only the connection
//  establishment differs. The maintainer must deploy/host the signaling relay (+ a
//  TURN server for symmetric NAT); this is a documented carryover mirroring the
//  Android `deploy/` bundle.
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
    /// The room code to share with the joiner (room-code host only, v1.9.7).
    @Published private(set) var hostedRoomCode: String?
    /// A transient error surfaced to the netplay UI.
    @Published var lastError: String?

    // MARK: - Room-code config (v1.9.7), persisted in UserDefaults; edited in Settings

    /// The signaling relay URL (`wss://` / `ws://`). REQUIRED for room-code play. The
    /// default is EMPTY so room-code play stays disabled until the user enters a real
    /// relay URL (the Settings field shows a placeholder as its prompt only).
    @Published var signalingURL: String = UserDefaults.standard.string(forKey: Keys.signaling)
        ?? "" {
        didSet { UserDefaults.standard.set(signalingURL, forKey: Keys.signaling) }
    }
    /// Optional STUN servers, one per line (`host:port`). Empty -> the bridge defaults.
    @Published var stunServers: String = UserDefaults.standard.string(forKey: Keys.stun) ?? "" {
        didSet { UserDefaults.standard.set(stunServers, forKey: Keys.stun) }
    }
    /// Optional TURN relay `host:port` (symmetric-NAT fallback). Empty -> no relay.
    @Published var turnURL: String = UserDefaults.standard.string(forKey: Keys.turnURL) ?? "" {
        didSet { UserDefaults.standard.set(turnURL, forKey: Keys.turnURL) }
    }
    /// The TURN username (required alongside `turnURL`).
    @Published var turnUser: String = UserDefaults.standard.string(forKey: Keys.turnUser) ?? "" {
        didSet { UserDefaults.standard.set(turnUser, forKey: Keys.turnUser) }
    }
    /// The TURN shared secret / password (required alongside `turnURL`). It is a
    /// credential, so it lives in the Keychain (NOT UserDefaults), like the RA token.
    @Published var turnSecret: String = Keychain.get(account: Keys.turnSecret) ?? "" {
        didSet { Keychain.set(turnSecret, account: Keys.turnSecret) }
    }

    private enum Keys {
        static let signaling = "netplay.signalingURL"
        static let stun = "netplay.stunServers"
        static let turnURL = "netplay.turnURL"
        static let turnUser = "netplay.turnUser"
        static let turnSecret = "netplay.turnSecret"
    }

    /// Whether the signaling URL is configured well enough to attempt room-code play.
    var signalingConfigured: Bool {
        // Trim newlines too so a trailing CR/LF from a copy-pasted URL doesn't slip
        // through (and later break the connection). `ws://` stays valid for LAN relays.
        let u = signalingURL.trimmingCharacters(in: .whitespacesAndNewlines)
        return u.hasPrefix("wss://") || u.hasPrefix("ws://")
    }

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
        // End any live session before dropping the core ref, so teardown is
        // deterministic and the peer is notified (a game close/swap mid-session).
        if let core, core.npIsActive() { core.npLeave() }
        core = nil
        status = nil
        hostedPort = nil
        hostedRoomCode = nil
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

    // MARK: - Host / Join by room code (CGNAT / internet, v1.9.7)

    /// Host a room-code session: register with the signaling relay, begin NAT
    /// traversal, and publish the returned room code in `hostedRoomCode` for the peer
    /// to enter. The frame loop then advances via `npAdvanceFrame` like the LAN path.
    func hostRoom() {
        guard let core else { lastError = "Open a game first to host netplay."; return }
        guard signalingConfigured else { lastError = signalingHint; return }
        do {
            hostedRoomCode = try core.npHostRoom(numPlayers: 2, cfg: makeNetConfig())
            hostedPort = nil
            lastError = nil
            startPolling()
        } catch {
            lastError = "Could not host room: \(error.localizedDescription)"
        }
    }

    /// Join a room-code session by its `code`.
    func joinRoom(code: String) {
        guard let core else { lastError = "Open a game first to join netplay."; return }
        guard signalingConfigured else { lastError = signalingHint; return }
        let trimmed = code.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { lastError = "Enter the room code."; return }
        do {
            try core.npJoinRoom(roomCode: trimmed, cfg: makeNetConfig())
            hostedPort = nil
            hostedRoomCode = nil
            lastError = nil
            startPolling()
        } catch {
            lastError = "Could not join room: \(error.localizedDescription)"
        }
    }

    private var signalingHint: String {
        "Set a signaling relay URL (wss://...) in Settings > Netplay before using room codes."
    }

    /// Build the bridge `NpNetConfig` from the persisted settings: a required signaling
    /// URL, an optional newline-separated STUN list (empty -> bridge defaults), and the
    /// optional TURN trio (all three required together, else the relay path is off).
    private func makeNetConfig() -> NpNetConfig {
        // Split on any newline (so Windows `\r\n` copy-paste parses) and trim each entry
        // of surrounding whitespace AND newlines (a stray `\r`).
        let stun = stunServers
            .components(separatedBy: .newlines)
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
        let url = turnURL.trimmingCharacters(in: .whitespacesAndNewlines)
        let user = turnUser.trimmingCharacters(in: .whitespacesAndNewlines)
        let secret = turnSecret.trimmingCharacters(in: .whitespacesAndNewlines)
        // Only configure TURN when the full trio is present; otherwise leave it nil so
        // the bridge runs punch-or-fail (cone-NAT only).
        let turnComplete = !url.isEmpty && !user.isEmpty && !secret.isEmpty
        return NpNetConfig(
            stunServers: stun,
            turnUrl: turnComplete ? url : nil,
            turnUser: turnComplete ? user : nil,
            turnSecret: turnComplete ? secret : nil,
            signalingUrl: signalingURL.trimmingCharacters(in: .whitespacesAndNewlines)
        )
    }

    /// Leave the session and return to single-player.
    func leave() {
        core?.npLeave()
        hostedPort = nil
        hostedRoomCode = nil
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
            // `ifa_addr` is nil for interfaces that are down / have no address; skip
            // them rather than force-dereferencing (which would crash).
            guard let addr = interface.ifa_addr else { ptr = interface.ifa_next; continue }
            let family = addr.pointee.sa_family
            if family == UInt8(AF_INET) {
                let name = String(cString: interface.ifa_name)
                // `en0` is the Wi-Fi interface on iPhone/iPad; LAN netplay rides Wi-Fi.
                if name == "en0" {
                    var host = [CChar](repeating: 0, count: Int(NI_MAXHOST))
                    if getnameinfo(
                        addr,
                        socklen_t(addr.pointee.sa_len),
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

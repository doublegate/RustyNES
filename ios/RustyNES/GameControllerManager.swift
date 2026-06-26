//
//  GameControllerManager.swift
//
//  Hardware-gamepad input via the GameController framework. The v1.9.2 rework lifts
//  this from a single-port binder to a P1-P4 multi-controller manager with a
//  user-editable button remap:
//
//    * Discovers up to four connected controllers (MFi / Xbox / DualShock / DualSense
//      / Backbone, etc.). Each is assigned a NES port (0-3) in connection order, with
//      a best-effort restore of a previously chosen port for the same controller model.
//    * Maps each controller's extended-gamepad profile onto the NES button bitmask
//      through a shared, persisted `ButtonRemap`. The extended buttons (X/Y, shoulders)
//      default to turbo-A / turbo-B; the standard inputs default to the desktop bind
//      (South=A, West=B, Menu=Start, Options=Select, D-pad/left-stick=directions).
//    * Feeds each controller's mask to `EmulatorCore.setButtons(port:mask:)` for its
//      assigned port via `onMaskChanged`. The bitmask path is unchanged, so the core
//      and determinism are untouched.
//
//  It is an `ObservableObject` so SettingsView can list controllers, reassign ports,
//  and edit the remap live. Mirrors the Android GamepadManager + remap.
//
//  Identity caveat (for the on-device dev): GameController exposes no stable hardware
//  UUID, so port persistence is keyed by `vendorName` (a best effort). Two identical
//  pads, or a renamed pad, fall back to connection order.
//

import Combine
import Foundation
import GameController

// MARK: - Remap model

/// A physical source on the extended-gamepad profile (what the user is mapping FROM).
enum PhysicalButton: String, Codable, CaseIterable, Identifiable {
    case south, east, west, north
    case l1, r1, l2, r2
    case menu, options
    case dpadUp, dpadDown, dpadLeft, dpadRight

    var id: String { rawValue }

    /// A human label (Xbox-style face names, the common shorthand for the rest).
    var label: String {
        switch self {
        case .south: return "A (South)"
        case .east: return "B (East)"
        case .west: return "X (West)"
        case .north: return "Y (North)"
        case .l1: return "L1 / LB"
        case .r1: return "R1 / RB"
        case .l2: return "L2 / LT"
        case .r2: return "R2 / RT"
        case .menu: return "Menu"
        case .options: return "Options"
        case .dpadUp: return "D-pad Up"
        case .dpadDown: return "D-pad Down"
        case .dpadLeft: return "D-pad Left"
        case .dpadRight: return "D-pad Right"
        }
    }
}

/// A NES target a physical button can drive (what the user is mapping TO), including
/// turbo (auto-fire) variants and an explicit unmapped option.
enum ControllerInput: String, Codable, CaseIterable, Identifiable {
    case a, b, select, start, up, down, left, right, turboA, turboB, none

    var id: String { rawValue }

    var label: String {
        switch self {
        case .a: return "A"
        case .b: return "B"
        case .select: return "Select"
        case .start: return "Start"
        case .up: return "Up"
        case .down: return "Down"
        case .left: return "Left"
        case .right: return "Right"
        case .turboA: return "Turbo A"
        case .turboB: return "Turbo B"
        case .none: return "Unmapped"
        }
    }

    /// The plain NES button this drives, or nil for turbo/unmapped (handled apart).
    var nesButton: NesButton? {
        switch self {
        case .a: return .a
        case .b: return .b
        case .select: return .select
        case .start: return .start
        case .up: return .up
        case .down: return .down
        case .left: return .left
        case .right: return .right
        case .turboA, .turboB, .none: return nil
        }
    }
}

/// The shared physical->NES mapping. One profile applies to every controller.
struct ButtonRemap: Codable, Equatable {
    var mapping: [PhysicalButton: ControllerInput]

    func target(for button: PhysicalButton) -> ControllerInput {
        mapping[button] ?? .none
    }

    /// The default bind, matching the desktop USB-gamepad layout. The unused extended
    /// buttons default to turbo so they are "real" out of the box, not no-ops.
    static let standard = ButtonRemap(mapping: [
        .south: .a, .west: .b,
        .east: .turboB, .north: .turboA,
        .menu: .start, .options: .select,
        .dpadUp: .up, .dpadDown: .down, .dpadLeft: .left, .dpadRight: .right,
        .l1: .none, .r1: .none, .l2: .none, .r2: .none,
    ])
}

/// A connected controller as surfaced to the UI (name + the port it drives).
struct ConnectedController: Identifiable {
    let id: ObjectIdentifier
    let name: String
    var port: Int
}

// MARK: - Manager

/// Bridges up to four connected hardware controllers to their NES ports.
final class GameControllerManager: ObservableObject {
    /// The maximum number of NES ports (P1-P4 / Four Score).
    static let maxPlayers = 4

    /// Called whenever a port's mask changes (port 0-3, raw 8-bit NES mask).
    var onMaskChanged: ((_ port: UInt32, _ mask: UInt8) -> Void)?

    /// The live controller list for the settings UI (name + assigned port).
    @Published private(set) var connected: [ConnectedController] = []

    /// The active remap profile; mutating it re-persists and re-evaluates input.
    @Published var remap: ButtonRemap {
        didSet {
            persistRemap()
            managed.forEach { evaluate($0) }
        }
    }

    /// Internal per-controller state (the bound pad, its port, and current masks).
    private final class Managed {
        let controller: GCController
        var port: Int
        var held = NesButtonMask()
        var turbo = NesButtonMask()
        init(controller: GCController, port: Int) {
            self.controller = controller
            self.port = port
        }
    }

    private var managed: [Managed] = []
    private var observers: [NSObjectProtocol] = []

    // Turbo (auto-fire) toggling: a single shared 30 Hz timer flips the phase, and
    // any port with turbo bits held re-emits so its A/B pulses.
    private var turboTimer: Timer?
    private var turboPhase = false

    private let remapKey = "controller.remap"
    private let portsKey = "controller.ports"

    init() {
        remap = GameControllerManager.loadRemap()
    }

    // MARK: Lifecycle

    func start() {
        let center = NotificationCenter.default
        observers.append(center.addObserver(
            forName: .GCControllerDidConnect, object: nil, queue: .main
        ) { [weak self] note in
            if let controller = note.object as? GCController { self?.connect(controller) }
        })
        observers.append(center.addObserver(
            forName: .GCControllerDidDisconnect, object: nil, queue: .main
        ) { [weak self] note in
            if let controller = note.object as? GCController { self?.disconnect(controller) }
        })
        // Bind anything already attached at launch.
        GCController.controllers().forEach(connect)
    }

    func stop() {
        observers.forEach { NotificationCenter.default.removeObserver($0) }
        observers.removeAll()
        turboTimer?.invalidate()
        turboTimer = nil
    }

    deinit { stop() }

    // MARK: Connect / disconnect

    private func connect(_ controller: GCController) {
        guard managed.count < Self.maxPlayers else { return }
        guard !managed.contains(where: { $0.controller === controller }) else { return }
        // Only manage controllers with a usable extended-gamepad profile (the path
        // `evaluate` reads). A controller without one (e.g. a Siri Remote) would
        // otherwise consume a player slot + a Settings row while producing no input,
        // and could block a real gamepad once `maxPlayers` is reached.
        guard controller.extendedGamepad != nil else { return }

        let port = preferredPort(for: controller)
        let m = Managed(controller: controller, port: port)
        managed.append(m)
        controller.playerIndex = GCControllerPlayerIndex(rawValue: port) ?? .indexUnset

        controller.extendedGamepad?.valueChangedHandler = { [weak self, weak m] _, _ in
            if let self, let m { self.evaluate(m) }
        }

        // Remember this controller's (possibly auto-assigned) port keyed by model name
        // so a later disconnect -> reconnect of the same pad reclaims the same port
        // where it is free (preferredPort consults this). Previously only an explicit
        // Settings reassignment persisted, so a hot-replugged pad could land on a
        // different port. A controller hot-plugged mid-game starts driving immediately
        // via the evaluate() below; no game state is touched.
        persistPorts()

        refresh()
        evaluate(m)
    }

    private func disconnect(_ controller: GCController) {
        guard let index = managed.firstIndex(where: { $0.controller === controller }) else { return }
        let port = managed[index].port
        managed.remove(at: index)
        // Release that port so its last-held buttons don't stick (a pad yanked
        // mid-frame must not leave a NES button latched). The port is now free for a
        // reconnecting pad to reclaim. The input path survives the drop: the bound
        // value handler held `m` weakly, so it no-ops once `m` is released here.
        onMaskChanged?(UInt32(port), 0)
        refresh()
        updateTurboTimer()
    }

    // MARK: Port assignment (user-driven, from Settings)

    /// Reassign which port a controller drives. If another controller already holds
    /// the target port, the two swap so ports stay unique.
    func assign(controllerID: ObjectIdentifier, toPort port: Int) {
        guard port >= 0, port < Self.maxPlayers,
              let target = managed.first(where: { ObjectIdentifier($0.controller) == controllerID })
        else { return }
        guard target.port != port else { return }

        let previous = target.port
        if let other = managed.first(where: { $0.port == port && $0.controller !== target.controller }) {
            other.port = previous
            other.controller.playerIndex = GCControllerPlayerIndex(rawValue: previous) ?? .indexUnset
            onMaskChanged?(UInt32(previous), 0)
        }
        onMaskChanged?(UInt32(port), 0)

        target.port = port
        target.controller.playerIndex = GCControllerPlayerIndex(rawValue: port) ?? .indexUnset

        // Re-emit both ports under their new owners, refresh UI, persist.
        managed.forEach { evaluate($0) }
        refresh()
        persistPorts()
    }

    // MARK: Input evaluation

    private func evaluate(_ m: Managed) {
        guard let pad = m.controller.extendedGamepad else { return }

        var held = NesButtonMask()
        var turbo = NesButtonMask()

        for physical in PhysicalButton.allCases where pressed(physical, on: pad) {
            let target = remap.target(for: physical)
            if let nes = target.nesButton {
                held.set(nes, pressed: true)
            } else if target == .turboA {
                turbo.set(.a, pressed: true)
            } else if target == .turboB {
                turbo.set(.b, pressed: true)
            }
        }

        // Left thumbstick -> D-pad (deadzoned) so analog-only pads still steer.
        let dz: Float = 0.5
        let stick = pad.leftThumbstick
        if stick.yAxis.value > dz { held.set(.up, pressed: true) }
        if stick.yAxis.value < -dz { held.set(.down, pressed: true) }
        if stick.xAxis.value < -dz { held.set(.left, pressed: true) }
        if stick.xAxis.value > dz { held.set(.right, pressed: true) }

        m.held = held
        m.turbo = turbo
        emit(m)
        updateTurboTimer()
    }

    /// Read a single physical button's pressed state off the extended profile.
    private func pressed(_ physical: PhysicalButton, on pad: GCExtendedGamepad) -> Bool {
        switch physical {
        case .south: return pad.buttonA.isPressed
        case .east: return pad.buttonB.isPressed
        case .west: return pad.buttonX.isPressed
        case .north: return pad.buttonY.isPressed
        case .l1: return pad.leftShoulder.isPressed
        case .r1: return pad.rightShoulder.isPressed
        case .l2: return pad.leftTrigger.isPressed
        case .r2: return pad.rightTrigger.isPressed
        case .menu: return pad.buttonMenu.isPressed
        case .options: return pad.buttonOptions?.isPressed ?? false
        case .dpadUp: return pad.dpad.up.isPressed
        case .dpadDown: return pad.dpad.down.isPressed
        case .dpadLeft: return pad.dpad.left.isPressed
        case .dpadRight: return pad.dpad.right.isPressed
        }
    }

    /// Push a port's current mask (held buttons, plus the turbo bits on the on-phase).
    private func emit(_ m: Managed) {
        var bits = m.held.bits
        if turboPhase { bits |= m.turbo.bits }
        onMaskChanged?(UInt32(m.port), bits)
    }

    // MARK: Turbo timer

    private func updateTurboTimer() {
        let anyTurbo = managed.contains { $0.turbo.bits != 0 }
        if anyTurbo, turboTimer == nil {
            // Schedule on the MAIN run loop in `.common` mode (not the implicit
            // `scheduledTimer`, which binds to the calling thread's run loop in
            // `.default` mode): GameController handlers can be delivered off-main,
            // and `.default` mode pauses the turbo pulse during UI tracking.
            let timer = Timer(timeInterval: 1.0 / 30.0, repeats: true) { [weak self] _ in
                guard let self else { return }
                self.turboPhase.toggle()
                for m in self.managed where m.turbo.bits != 0 { self.emit(m) }
            }
            RunLoop.main.add(timer, forMode: .common)
            turboTimer = timer
        } else if !anyTurbo, turboTimer != nil {
            turboTimer?.invalidate()
            turboTimer = nil
            turboPhase = false
            // Settle every port back to its plain held mask.
            managed.forEach { emit($0) }
        }
    }

    // MARK: UI mirror + persistence

    private func refresh() {
        connected = managed
            .sorted { $0.port < $1.port }
            .map {
                ConnectedController(
                    id: ObjectIdentifier($0.controller),
                    name: $0.controller.vendorName ?? "Controller",
                    port: $0.port
                )
            }
    }

    /// The port to give a newly connected controller: its remembered port if free,
    /// else the lowest unused port.
    private func preferredPort(for controller: GCController) -> Int {
        let used = Set(managed.map { $0.port })
        if let name = controller.vendorName,
           let saved = savedPorts()[name], !used.contains(saved), saved < Self.maxPlayers {
            return saved
        }
        return (0..<Self.maxPlayers).first { !used.contains($0) } ?? 0
    }

    private func savedPorts() -> [String: Int] {
        UserDefaults.standard.dictionary(forKey: portsKey) as? [String: Int] ?? [:]
    }

    private func persistPorts() {
        var ports = savedPorts()
        for m in managed {
            if let name = m.controller.vendorName { ports[name] = m.port }
        }
        UserDefaults.standard.set(ports, forKey: portsKey)
    }

    private func persistRemap() {
        if let data = try? JSONEncoder().encode(remap) {
            UserDefaults.standard.set(data, forKey: remapKey)
        }
    }

    private static func loadRemap() -> ButtonRemap {
        guard let data = UserDefaults.standard.data(forKey: "controller.remap"),
              let decoded = try? JSONDecoder().decode(ButtonRemap.self, from: data)
        else { return .standard }
        return decoded
    }
}

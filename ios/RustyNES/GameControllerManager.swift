//
//  GameControllerManager.swift
//
//  Hardware-gamepad input via the GameController framework. Discovers connected
//  controllers (MFi / Xbox / DualShock / DualSense / Backbone, etc.), maps the
//  extended-gamepad profile onto the NES button bitmask, and feeds it to the
//  EmulatorCore. Mirrors the Android GamepadManager.
//
//  Mapping (Xbox-style face layout, matching the desktop USB-gamepad bind):
//      South (A) -> NES A,  West (X) -> NES B,
//      Menu/Start -> Start, Options/Select -> Select, D-pad -> D-pad.
//  The left thumbstick also feeds the D-pad (deadzoned) for sticks-only pads.
//

import GameController
import Foundation

/// Bridges connected hardware controllers to a port's NES button mask.
final class GameControllerManager {
    /// Called whenever the mask for the bound port changes.
    var onMaskChanged: ((_ port: UInt32, _ mask: UInt8) -> Void)?

    /// The port this manager drives (player 1 by default).
    var port: UInt32 = 0

    private var current = NesButtonMask()
    private var observers: [NSObjectProtocol] = []

    func start() {
        let center = NotificationCenter.default
        observers.append(center.addObserver(
            forName: .GCControllerDidConnect, object: nil, queue: .main
        ) { [weak self] note in
            if let controller = note.object as? GCController { self?.bind(controller) }
        })
        observers.append(center.addObserver(
            forName: .GCControllerDidDisconnect, object: nil, queue: .main
        ) { [weak self] _ in
            // A disconnect releases everything so no button sticks.
            self?.current.clear()
            self?.emit()
        })
        // Bind any already-connected controllers.
        GCController.controllers().forEach(bind)
    }

    func stop() {
        observers.forEach { NotificationCenter.default.removeObserver($0) }
        observers.removeAll()
    }

    private func bind(_ controller: GCController) {
        guard let pad = controller.extendedGamepad else { return }
        pad.valueChangedHandler = { [weak self] gamepad, _ in
            self?.update(from: gamepad)
        }
    }

    private func update(from pad: GCExtendedGamepad) {
        var mask = NesButtonMask()

        // Face buttons (Xbox-style; matches the desktop bind).
        mask.set(.a, pressed: pad.buttonA.isPressed)
        mask.set(.b, pressed: pad.buttonX.isPressed)

        // Menu = Start, Options = Select (fall back to the shoulder-area buttons
        // on pads that only expose one system button).
        mask.set(.start, pressed: pad.buttonMenu.isPressed)
        if let options = pad.buttonOptions {
            mask.set(.select, pressed: options.isPressed)
        }

        // D-pad.
        mask.set(.up, pressed: pad.dpad.up.isPressed)
        mask.set(.down, pressed: pad.dpad.down.isPressed)
        mask.set(.left, pressed: pad.dpad.left.isPressed)
        mask.set(.right, pressed: pad.dpad.right.isPressed)

        // Left thumbstick -> D-pad (deadzoned) so analog-only pads still steer.
        let dz: Float = 0.5
        let stick = pad.leftThumbstick
        if stick.yAxis.value > dz { mask.set(.up, pressed: true) }
        if stick.yAxis.value < -dz { mask.set(.down, pressed: true) }
        if stick.xAxis.value < -dz { mask.set(.left, pressed: true) }
        if stick.xAxis.value > dz { mask.set(.right, pressed: true) }

        if mask.bits != current.bits {
            current = mask
            emit()
        }
    }

    private func emit() {
        onMaskChanged?(port, current.bits)
    }

    deinit { stop() }
}

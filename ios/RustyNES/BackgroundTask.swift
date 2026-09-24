//
//  BackgroundTask.swift
//
//  A UIKit background task that always ends (v2.7.4, frontend audit IOS-04).
//
//  Work started as the app leaves the foreground -- a CloudKit upload, a battery
//  save -- is suspended with the process unless it runs inside a background task.
//  The catch is the expiration handler: if the time the system grants runs out
//  and the task has not been ended, iOS terminates the app. So every task begun
//  here ends exactly once, whichever comes first -- the work finishing, or the
//  expiration handler firing.
//

import UIKit

@MainActor
final class BackgroundTask {
    private var id: UIBackgroundTaskIdentifier = .invalid

    /// Begin a background task named `name` (shown in debugging tools).
    init(name: String) {
        id = UIApplication.shared.beginBackgroundTask(withName: name) { [weak self] in
            // The expiration handler runs on the main thread.
            MainActor.assumeIsolated { self?.end() }
        }
    }

    /// End the task. Safe to call more than once; only the first call ends it.
    func end() {
        guard id != .invalid else { return }
        UIApplication.shared.endBackgroundTask(id)
        id = .invalid
    }
}

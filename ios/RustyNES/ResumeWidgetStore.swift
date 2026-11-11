//
//  ResumeWidgetStore.swift
//
//  v3.7.0 (D24, `T-MOBILE-EXTRAS` row 1): the app's side of the home-screen widget.
//  When a game opens, record it in the App Group (ResumeWidgetShared) and ask
//  WidgetKit to redraw the widget. UNCOMPILED at v3.7.0 (iOS CI compiles it).
//

import Foundation
import WidgetKit

enum ResumeWidgetStore {
    /// Record the game that just opened and reload the widget's timeline.
    static func publish(name: String, sha: String) {
        ResumeWidgetShared.write(.init(name: name, sha: sha, played: Date()))
        WidgetCenter.shared.reloadTimelines(ofKind: ResumeWidgetShared.kind)
    }
}

import Foundation

public enum OverlayMode: Hashable, Sendable {
    case collapsed
    case expanded(Timeframe)

    public var name: String {
        switch self {
        case .collapsed: "collapsed"
        case .expanded: "expanded"
        }
    }
}

public enum Frontmost: Hashable, Sendable, CaseIterable {
    case wow
    /// This app, which is frontmost only while its Settings window is in use.
    case ticker
    case other

    public static let wowBundleID = "com.blizzard.worldofwarcraft"
}

/// A press becomes a drag only once the pointer travels `OverlayState.dragThreshold` on an unlocked overlay.
public enum DragPhase: Hashable, Sendable {
    case idle, pressed, dragging
}

/// What is under the pointer when the mouse comes up.
public enum PressTarget: Hashable, Sendable {
    /// Anywhere on the overlay outside the timeframe controls.
    case body
    case timeframe(Timeframe)
    case outside
}

public struct OverlayState: Hashable, Sendable {
    public var settings: Settings
    public var frontmost: Frontmost
    public var mode: OverlayMode = .collapsed
    public var drag: DragPhase = .idle

    public init(settings: Settings, frontmost: Frontmost) {
        self.settings = settings
        self.frontmost = frontmost
    }

    /// The ticker counts as "WoW active" too, so the overlay stays visible while its own Settings window is open.
    public var isVisible: Bool { !settings.onlyWhenWoWActive || frontmost != .other }
}

public enum OverlayEvent: Hashable, Sendable {
    case pressed
    /// The pointer is `distance` points from where the mouse went down.
    case pointerMoved(distance: Double)
    /// A drag drops the ticker line at `tickerOrigin`. Any other press is a click: on the body it toggles
    /// between the ticker and the chart, on a timeframe control it selects that timeframe.
    case released(over: PressTarget, tickerOrigin: ScreenPoint)
    case toggleLock
    case appActivated(Frontmost)
    case settingsEdited(Settings)
}

extension OverlayState {
    public static let dragThreshold = 3.0

    public static func reduce(_ state: OverlayState, _ event: OverlayEvent) -> OverlayState {
        var next = state
        switch event {
        case .pressed:
            next.drag = .pressed
        case .pointerMoved(let distance):
            if state.drag == .pressed, !state.settings.locked, distance >= dragThreshold { next.drag = .dragging }
        case .released(let target, let origin):
            next.drag = .idle
            switch (state.drag, target, state.mode) {
            case (.dragging, _, _):
                next.settings.position = origin
            case (.pressed, .body, .collapsed):
                next.mode = .expanded(state.settings.timeframe)
            case (.pressed, .body, .expanded):
                next.mode = .collapsed
            case (.pressed, .timeframe(let timeframe), .expanded):
                next.settings.timeframe = timeframe
            default:
                break
            }
        case .toggleLock:
            next.settings.locked.toggle()
        case .appActivated(let app):
            next.frontmost = app
        case .settingsEdited(let settings):
            next.settings = settings
        }
        if case .expanded = next.mode {
            next.mode = .expanded(next.settings.timeframe)
        }
        if !next.isVisible || (next.settings.locked && next.drag == .dragging) {
            next.drag = .idle
        }
        return next
    }
}

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

public enum DragPhase: Hashable, Sendable {
    case idle, dragging
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
    /// Toggles between the ticker and the chart.
    case click
    /// Only meaningful while expanded.
    case selectTimeframe(Timeframe)
    case dragBegan
    case dragEnded(at: ScreenPoint)
    case toggleLock
    case appActivated(Frontmost)
    case settingsEdited(Settings)
}

extension OverlayState {
    public static func reduce(_ state: OverlayState, _ event: OverlayEvent) -> OverlayState {
        var next = state
        switch event {
        case .click:
            next.mode = state.mode == .collapsed ? .expanded(state.settings.timeframe) : .collapsed
        case .selectTimeframe(let timeframe):
            if case .expanded = state.mode { next.settings.timeframe = timeframe }
        case .dragBegan:
            next.drag = .dragging
        case .dragEnded(let origin):
            if state.drag == .dragging {
                next.settings.position = origin
                next.drag = .idle
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
        if next.settings.locked || !next.isVisible {
            next.drag = .idle
        }
        return next
    }
}

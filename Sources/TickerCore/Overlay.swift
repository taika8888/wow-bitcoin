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

public enum Frontmost: String, Codable, Hashable, Sendable, CaseIterable {
    case wow
    /// This app, which is frontmost only while its Settings window is in use.
    case ticker
    case other

    public static let wowBundleID = "com.blizzard.worldofwarcraft"
}

/// Interaction state supplied by the shared core.
public enum DragPhase: String, Decodable, Hashable, Sendable {
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
    public let settings: Settings
    public let mode: OverlayMode
    public let drag: DragPhase
    public let isVisible: Bool
}

public enum OverlayEvent: Hashable, Sendable {
    case pressed
    /// The pointer is `distance` points from where the mouse went down.
    case pointerMoved(distance: Double)
    /// A drag drops the ticker line at `tickerOrigin`. Any other press is a click: on the body it toggles
    /// between the ticker and the chart, on a timeframe control it selects that timeframe.
    case released(over: PressTarget, tickerOrigin: ScreenPoint)
    case toggleLock
    /// Steps the chart one rung along the zoom ladder, and only while the chart is on screen.
    case zoom(Zoom.Step)
    case appActivated(Frontmost)
    case settingsEdited(Settings)
}

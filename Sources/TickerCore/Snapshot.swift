import Foundation

/// What the app writes to `BTCTICKER_STATE_FILE` after every change, for the end-to-end check.
public struct Snapshot: Codable, Hashable, Sendable {
    /// Global display coordinates with a top-left origin, the same space as `CGWindowListCopyWindowInfo` bounds.
    public struct Frame: Codable, Hashable, Sendable {
        public var x: Double
        public var y: Double
        public var width: Double
        public var height: Double

        public init(x: Double, y: Double, width: Double, height: Double) {
            self.x = x
            self.y = y
            self.width = width
            self.height = height
        }
    }

    public var text: String
    public var feed: String
    /// When the quote on screen was fetched, in seconds since 1970.
    public var fetchedAt: Double?
    public var mode: String
    public var locked: Bool
    public var opacity: Double
    public var visible: Bool
    public var windowNumber: Int
    public var frame: Frame
    /// The rest describe the chart and are absent or zero while collapsed.
    public var timeframe: String?
    public var chart: String?
    public var candles: Int
    /// Click targets in the same space as `frame`: "ticker" always, and each timeframe label while expanded.
    public var targets: [String: Frame]
    /// How often this app became the active app. Clicking the overlay must never raise it.
    public var activations: Int
    /// The main menu's shortcuts as "cmd-<key> <action>", so the check can see Close and Quit without typing.
    public var shortcuts: [String] = []

    public init(
        display: TickerDisplay, status: FeedStatus, overlay: OverlayState, chart: CandleLoad?, candles: Int,
        windowNumber: Int, frame: Frame, targets: [String: Frame], activations: Int
    ) {
        text = display.text
        feed = status.name
        if case .live(let quote) = status { fetchedAt = quote.fetchedAt.timeIntervalSince1970 }
        if case .stale(let quote) = status { fetchedAt = quote.fetchedAt.timeIntervalSince1970 }
        mode = overlay.mode.name
        locked = overlay.settings.locked
        opacity = overlay.settings.opacity
        visible = overlay.isVisible
        if case .expanded(let timeframe) = overlay.mode { self.timeframe = timeframe.spec.label }
        self.chart = chart?.name
        self.candles = candles
        self.windowNumber = windowNumber
        self.frame = frame
        self.targets = targets
        self.activations = activations
    }

    public var json: Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = .sortedKeys
        return try! encoder.encode(self)
    }
}

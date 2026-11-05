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
    public var mode: String
    public var locked: Bool
    public var visible: Bool
    public var windowNumber: Int
    public var frame: Frame
    /// The rest describe the chart and are absent or zero while collapsed.
    public var timeframe: String?
    public var chart: String?
    public var candles: Int
    /// Click targets in the same space as `frame`: "ticker" always, and each timeframe label while expanded.
    public var targets: [String: Frame]

    public init(
        display: TickerDisplay, status: FeedStatus, overlay: OverlayState, chart: CandleLoad?, candles: Int,
        windowNumber: Int, frame: Frame, targets: [String: Frame]
    ) {
        text = display.text
        feed = status.name
        mode = overlay.mode.name
        locked = overlay.settings.locked
        visible = overlay.isVisible
        if case .expanded(let timeframe) = overlay.mode { self.timeframe = timeframe.spec.label }
        self.chart = chart?.name
        self.candles = candles
        self.windowNumber = windowNumber
        self.frame = frame
        self.targets = targets
    }

    public var json: Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = .sortedKeys
        return try! encoder.encode(self)
    }
}

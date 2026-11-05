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

    public init(display: TickerDisplay, status: FeedStatus, overlay: OverlayState, windowNumber: Int, frame: Frame) {
        text = display.text
        feed = status.name
        mode = overlay.mode.name
        locked = overlay.settings.locked
        visible = overlay.isVisible
        self.windowNumber = windowNumber
        self.frame = frame
    }

    public var json: Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = .sortedKeys
        return try! encoder.encode(self)
    }
}

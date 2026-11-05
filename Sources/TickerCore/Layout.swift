import CoreGraphics

/// The expanded overlay in AppKit screen coordinates (bottom-left origin).
/// The ticker line keeps the collapsed ticker's exact screen rect, and the chart opens toward the middle of the screen.
public struct ExpandedLayout: Equatable, Sendable {
    public static let inset = 8.0
    public static let gap = 8.0

    public let opensDown: Bool
    public let opensRight: Bool
    /// The panel's frame on screen, clamped into the screen only when the screen is too small to keep the ticker put.
    public let frame: CGRect
    /// The rest are in panel coordinates.
    public let ticker: CGRect
    public let timeframes: [Timeframe: CGRect]
    public let chart: CGRect

    public init(tickerFrame: CGRect, segment: CGSize, chart chartSize: CGSize, screen: CGRect) {
        let inset = Self.inset
        let segmentsWidth = segment.width * Double(Timeframe.allCases.count)
        let width = max(tickerFrame.width + Self.gap + segmentsWidth + inset, chartSize.width + inset * 2)
        let height = tickerFrame.height + chartSize.height + inset
        opensDown = tickerFrame.midY > screen.midY
        opensRight = tickerFrame.midX < screen.midX

        ticker = CGRect(
            x: opensRight ? 0 : width - tickerFrame.width,
            y: opensDown ? height - tickerFrame.height : 0,
            width: tickerFrame.width,
            height: tickerFrame.height
        )
        let segmentsX = opensRight ? ticker.maxX + Self.gap : ticker.minX - Self.gap - segmentsWidth
        timeframes = Dictionary(uniqueKeysWithValues: Timeframe.allCases.enumerated().map { [ticker] index, timeframe in
            (timeframe, CGRect(x: segmentsX + Double(index) * segment.width, y: ticker.midY - segment.height / 2, width: segment.width, height: segment.height))
        })
        chart = CGRect(x: inset, y: opensDown ? inset : ticker.maxY, width: width - inset * 2, height: chartSize.height)

        let x = tickerFrame.minX - ticker.minX
        let y = tickerFrame.minY - ticker.minY
        frame = CGRect(
            x: max(min(x, screen.maxX - width), screen.minX),
            y: max(min(y, screen.maxY - height), screen.minY),
            width: width,
            height: height
        )
    }

    public func timeframe(at point: CGPoint) -> Timeframe? {
        Timeframe.allCases.first { timeframes[$0]?.contains(point) == true }
    }
}

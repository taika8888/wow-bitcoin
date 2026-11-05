import CoreGraphics

/// The expanded overlay in AppKit screen coordinates (bottom-left origin).
/// The ticker line keeps the collapsed ticker's exact screen rect, and the chart opens toward the middle of the screen.
public struct ExpandedLayout: Equatable, Sendable {
    public static let inset = 8.0
    public static let gap = 8.0
    /// Room the chart leaves for its trailing price labels.
    public static let priceAxisWidth = 60.0
    /// The narrowest a candlestick gets before neighbours merge, which caps the marks the chart draws at any zoom.
    public static let pointsPerBar = 2.0

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

    public var plotWidth: Double { chart.width - Self.priceAxisWidth }

    public var maxBars: Int { max(1, Int(plotWidth / Self.pointsPerBar)) }

    public func timeframe(at point: CGPoint) -> Timeframe? {
        Timeframe.allCases.first { timeframes[$0]?.contains(point) == true }
    }
}

/// One display in AppKit screen coordinates: its whole frame, and the part the menu bar and Dock leave free.
public struct ScreenArea: Hashable, Sendable {
    public var frame: CGRect
    public var visible: CGRect

    public init(frame: CGRect, visible: CGRect) {
        self.frame = frame
        self.visible = visible
    }
}

/// Where the collapsed ticker sits on screen, in AppKit screen coordinates.
public enum TickerPlacement {
    public static let margin = CGSize(width: 16, height: 12)

    /// The screen the rect overlaps most, or nil when it overlaps none.
    public static func screen(for rect: CGRect, in screens: [ScreenArea]) -> ScreenArea? {
        let overlaps = screens.map { screen in (screen, rect.intersection(screen.frame)) }.filter { !$0.1.isNull }
        return overlaps.max { $0.1.width * $0.1.height < $1.1.width * $1.1.height }?.0
    }

    /// Moves the rect the least distance that puts it inside the area; one larger than the area keeps its left and top edges in.
    public static func clamp(_ rect: CGRect, into area: CGRect) -> CGRect {
        CGRect(
            x: max(min(rect.minX, area.maxX - rect.width), area.minX),
            y: min(max(rect.minY, area.minY), area.maxY - rect.height),
            width: rect.width,
            height: rect.height
        )
    }

    /// The saved origin, kept whole inside the visible area of the screen the ticker overlaps most, or the
    /// top center of the first screen (WoW keeps its minimap and buffs top right) when nothing is saved or the saved spot is on no screen.
    public static func frame(saved: ScreenPoint?, size: CGSize, screens: [ScreenArea]) -> CGRect {
        if let saved {
            let rect = CGRect(origin: CGPoint(x: saved.x, y: saved.y), size: size)
            if let area = screen(for: rect, in: screens)?.visible { return clamp(rect, into: area) }
        }
        guard let area = screens.first?.visible else { return CGRect(origin: .zero, size: size) }
        return CGRect(origin: CGPoint(x: (area.midX - size.width / 2).rounded(), y: area.maxY - size.height - margin.height), size: size)
    }
}

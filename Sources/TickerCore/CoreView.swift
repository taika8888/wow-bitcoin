import Foundation

public enum Tone: String, Decodable, Hashable, Sendable { case neutral, up, down }

public struct DisplaySegment: Decodable, Hashable, Sendable {
    public let text: String
    public let tone: Tone
}

public struct TickerDisplay: Decodable, Hashable, Sendable {
    public let segments: [DisplaySegment]
    public let dimmed: Bool
    public var text: String { segments.map(\.text).joined() }
}

public struct FeedView: Decodable, Hashable, Sendable {
    public let name: String
    public let fetchedAt: Date?
}

public struct ChartNotice: Decodable, Hashable, Sendable {
    public enum Kind: String, Decodable, Sendable { case placeholder, stale }
    public let kind: Kind
    public let text: String
}

public struct CandleLoad: Decodable, Hashable, Sendable {
    public let name: String
    public let notice: ChartNotice?
}

public struct CandleChart: Decodable, Hashable, Sendable {
    public struct Bar: Decodable, Hashable, Sendable {
        public let time: Date
        public let low: Double
        public let high: Double
        public let bodyLow: Double
        public let bodyHigh: Double
        public let tone: Tone
    }

    public struct PriceTick: Decodable, Hashable, Sendable {
        public let value: Double
        public let label: String
    }

    public let bars: [Bar]
    private let prices: [Double]
    private let times: [Date]
    public let lastPrice: Double
    public let candleCount: Int
    public let priceTicks: [PriceTick]
    public let timeTicks: [Date]
    public var priceDomain: ClosedRange<Double> { prices[0]...prices[1] }
    public var timeDomain: ClosedRange<Date> { times[0]...times[1] }

    private enum CodingKeys: String, CodingKey {
        case bars, prices = "priceDomain", times = "timeDomain", lastPrice, candleCount, priceTicks, timeTicks
    }
}

public struct CoreView: Decodable, Sendable {
    public let settings: Settings
    private let overlay: Overlay
    public let status: FeedView
    public let display: TickerDisplay
    public let load: CandleLoad?
    public let chartRevision: UInt64
    public let chartStart: Date?
    public let chart: CandleChart?

    private struct Overlay: Decodable, Sendable {
        let mode: String
        let timeframe: Timeframe?
        let drag: DragPhase
        let isVisible: Bool
    }

    public var overlayState: OverlayState {
        OverlayState(settings: settings, mode: overlay.timeframe.map(OverlayMode.expanded) ?? .collapsed,
                     drag: overlay.drag, isVisible: overlay.isVisible)
    }
}

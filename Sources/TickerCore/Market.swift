import Foundation

public struct Quote: Hashable, Sendable {
    public let price: Double
    public let open24h: Double
    public let fetchedAt: Date

    public init(price: Double, open24h: Double, fetchedAt: Date) {
        self.price = price
        self.open24h = open24h
        self.fetchedAt = fetchedAt
    }

    /// Fraction of the open, so 0.01 means +1%.
    public var change24h: Double { (price - open24h) / open24h }
}

public struct Candle: Hashable, Sendable {
    public let time: Date
    public let open: Double
    public let high: Double
    public let low: Double
    public let close: Double

    public init(time: Date, open: Double, high: Double, low: Double, close: Double) {
        self.time = time
        self.open = open
        self.high = high
        self.low = low
        self.close = close
    }
}

public enum Timeframe: String, CaseIterable, Codable, Hashable, Sendable {
    case m1, m15, h4

    public struct Spec: Hashable, Sendable {
        public let label: String
        public let krakenIntervalMinutes: Int
        /// How many of the newest candles the chart shows at `Zoom.standard`.
        public let candlesShown: Int
        public let refreshSeconds: Int
    }

    public var spec: Spec {
        switch self {
        case .m1: Spec(label: "1m", krakenIntervalMinutes: 1, candlesShown: 120, refreshSeconds: 15)
        case .m15: Spec(label: "15m", krakenIntervalMinutes: 15, candlesShown: 96, refreshSeconds: 60)
        case .h4: Spec(label: "4h", krakenIntervalMinutes: 240, candlesShown: 90, refreshSeconds: 300)
        }
    }
}

/// A rung on the chart's zoom ladder. Each one shows a multiple of the timeframe's `candlesShown`, or every candle held.
public enum Zoom: Int, CaseIterable, Codable, Hashable, Sendable {
    case quarter, half, standard, double, quadruple, all

    public enum Step: Hashable, Sendable {
        /// Fewer, wider candles.
        case `in`
        case out
    }

    public var label: String {
        switch self {
        case .quarter: "×1/4"
        case .half: "×1/2"
        case .standard: "×1"
        case .double: "×2"
        case .quadruple: "×4"
        case .all: "all"
        }
    }

    /// One rung toward the step's direction, staying put at either end of the ladder.
    public func stepped(_ step: Step) -> Zoom {
        Zoom(rawValue: rawValue + (step == .in ? -1 : 1)) ?? self
    }

    /// How many of the newest candles to show out of `available`, at least one when there are any.
    public func visibleCount(_ timeframe: Timeframe, available: Int) -> Int {
        let shown = timeframe.spec.candlesShown
        let wanted = switch self {
        case .quarter: shown / 4
        case .half: shown / 2
        case .standard: shown
        case .double: shown * 2
        case .quadruple: shown * 4
        case .all: available
        }
        return min(max(wanted, 1), available)
    }
}

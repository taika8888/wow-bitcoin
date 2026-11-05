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
        public let candlesKept: Int
    }

    public var spec: Spec {
        switch self {
        case .m1: Spec(label: "1m", krakenIntervalMinutes: 1, candlesKept: 120)
        case .m15: Spec(label: "15m", krakenIntervalMinutes: 15, candlesKept: 96)
        case .h4: Spec(label: "4h", krakenIntervalMinutes: 240, candlesKept: 90)
        }
    }
}

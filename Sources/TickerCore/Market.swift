public enum Timeframe: String, CaseIterable, Codable, Hashable, Sendable {
    case m1, m15, h4

    public var label: String {
        switch self {
        case .m1: "1m"
        case .m15: "15m"
        case .h4: "4h"
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

}

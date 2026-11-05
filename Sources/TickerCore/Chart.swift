import Foundation

public struct CandleSeries: Hashable, Sendable {
    public let candles: [Candle]
    public let fetchedAt: Date

    public init(candles: [Candle], fetchedAt: Date) {
        self.candles = candles
        self.fetchedAt = fetchedAt
    }
}

/// One timeframe's candles. `loading` and `failed` keep the last good series so a refresh never blanks the chart.
public enum CandleLoad: Hashable, Sendable {
    case idle
    case loading(last: CandleSeries?)
    case loaded(CandleSeries)
    case failed(last: CandleSeries?, at: Date)

    public static let retryAfterFailure: TimeInterval = 10

    public var series: CandleSeries? {
        switch self {
        case .idle: nil
        case .loading(let last), .failed(let last, _): last
        case .loaded(let series): series
        }
    }

    public var name: String {
        switch self {
        case .idle: "idle"
        case .loading: "loading"
        case .loaded: "loaded"
        case .failed: "failed"
        }
    }

    func isDue(_ timeframe: Timeframe, now: Date) -> Bool {
        switch self {
        case .idle: true
        case .loading: false
        case .loaded(let series): now.timeIntervalSince(series.fetchedAt) >= TimeInterval(timeframe.spec.refreshSeconds)
        case .failed(_, let at): now.timeIntervalSince(at) >= Self.retryAfterFailure
        }
    }
}

/// Decides which timeframe to fetch: only the one on screen, only when due, never while its last request is in flight.
public struct ChartFeed: Hashable, Sendable {
    private var loads: [Timeframe: CandleLoad] = [:]

    public init() {}

    public subscript(timeframe: Timeframe) -> CandleLoad {
        loads[timeframe] ?? .idle
    }

    /// Marks the timeframe loading and returns it when a request should start now.
    public mutating func beginFetch(for overlay: OverlayState, now: Date) -> Timeframe? {
        guard overlay.isVisible, case .expanded(let timeframe) = overlay.mode, self[timeframe].isDue(timeframe, now: now) else {
            return nil
        }
        loads[timeframe] = .loading(last: self[timeframe].series)
        return timeframe
    }

    /// `nil` candles means the request failed.
    public mutating func finish(_ timeframe: Timeframe, candles: [Candle]?, at now: Date) {
        guard case .loading(let last) = self[timeframe] else { return }
        loads[timeframe] = candles.map { .loaded(CandleSeries(candles: $0, fetchedAt: now)) } ?? .failed(last: last, at: now)
    }
}

/// What the chart view draws. Swift Charts owns scales and axes; this owns which values go in them.
public struct CandleChart: Hashable, Sendable {
    public struct Bar: Hashable, Sendable {
        /// The middle of the candle's interval, so bars sit between interval boundaries.
        public let time: Date
        public let low: Double
        public let high: Double
        /// Open and close ordered, widened to a minimum height so a flat candle still shows.
        public let bodyLow: Double
        public let bodyHigh: Double
        public let tone: Tone
    }

    /// Fraction of the price range added above the highest high and below the lowest low.
    public static let pricePadding = 0.06
    /// Minimum body height as a fraction of the price domain.
    public static let minimumBody = 0.004

    public let bars: [Bar]
    public let priceDomain: ClosedRange<Double>
    public let timeDomain: ClosedRange<Date>
    public let lastPrice: Double

    public init?(candles: [Candle], timeframe: Timeframe) {
        guard let first = candles.first, let last = candles.last else { return nil }
        let low = candles.map(\.low).min()!
        let high = candles.map(\.high).max()!
        let range = high > low ? high - low : high * 0.001
        priceDomain = (low - range * Self.pricePadding)...(high + range * Self.pricePadding)
        let interval = TimeInterval(timeframe.spec.krakenIntervalMinutes * 60)
        timeDomain = first.time...last.time.addingTimeInterval(interval)
        lastPrice = last.close

        let minimumBody = (priceDomain.upperBound - priceDomain.lowerBound) * Self.minimumBody
        bars = candles.map { candle in
            let middle = (candle.open + candle.close) / 2
            let half = max(abs(candle.close - candle.open), minimumBody) / 2
            return Bar(
                time: candle.time.addingTimeInterval(interval / 2),
                low: candle.low,
                high: candle.high,
                bodyLow: middle - half,
                bodyHigh: middle + half,
                tone: candle.close > candle.open ? .up : candle.close < candle.open ? .down : .neutral
            )
        }
    }
}

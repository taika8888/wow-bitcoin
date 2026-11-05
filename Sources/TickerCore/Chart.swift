import Foundation

public struct CandleSeries: Hashable, Sendable {
    public let candles: [Candle]
    public let fetchedAt: Date

    public init(candles: [Candle], fetchedAt: Date) {
        self.candles = candles
        self.fetchedAt = fetchedAt
    }
}

public enum FetchFailure: Error, Hashable, Sendable {
    /// No answer at all: offline, timed out, refused.
    case unreachable
    case status(Int)
    /// Kraken answered 200 with a body that is not a usable response.
    case rejected(KrakenError)
}

extension FetchFailure {
    var message: String {
        switch self {
        case .unreachable: "Kraken is not answering"
        case .status(let code): "Kraken answered HTTP \(code)"
        case .rejected(.api(let errors)): "Kraken refused: \(errors.joined(separator: ", "))"
        case .rejected: "Kraken sent candles this app cannot read"
        }
    }
}

public struct FailedFetch: Hashable, Sendable {
    public let kind: FetchFailure
    public let at: Date
}

public enum ChartNotice: Hashable, Sendable {
    /// In place of the chart, when there are no candles to draw.
    case placeholder(String)
    /// A small marker over candles that the last refresh failed to update.
    case stale(String)
}

/// One timeframe's candles. A refresh in flight or a failed one keeps the last good series, so the chart never blanks.
public struct CandleLoad: Hashable, Sendable {
    public static let idle = CandleLoad()
    public static let retryAfterFailure: TimeInterval = 10

    public fileprivate(set) var series: CandleSeries?
    public fileprivate(set) var inFlight = false
    /// The last finished request's failure, cleared by the next success.
    public fileprivate(set) var failure: FailedFetch?

    public var name: String {
        if inFlight { return "loading" }
        if failure != nil { return "failed" }
        return series == nil ? "idle" : "loaded"
    }

    public func notice(for timeframe: Timeframe) -> ChartNotice? {
        let hasCandles = series?.candles.isEmpty == false
        switch (failure?.kind, hasCandles) {
        case (let kind?, true): return .stale("Stale: \(kind.message)")
        case (let kind?, false): return .placeholder("\(kind.message), retrying")
        case (nil, true): return nil
        case (nil, false): return .placeholder(series == nil ? "Loading \(timeframe.spec.label) candles" : "No \(timeframe.spec.label) candles")
        }
    }

    func isDue(_ timeframe: Timeframe, now: Date) -> Bool {
        if inFlight { return false }
        if let failure { return now.timeIntervalSince(failure.at) >= Self.retryAfterFailure }
        guard let series else { return true }
        return now.timeIntervalSince(series.fetchedAt) >= TimeInterval(timeframe.spec.refreshSeconds)
    }
}

/// Decides which timeframe to fetch: only the one on screen, only when due, never while its last request is in flight.
public struct ChartFeed: Hashable, Sendable {
    private var loads: [Timeframe: CandleLoad] = [:]

    public init() {}

    public subscript(timeframe: Timeframe) -> CandleLoad {
        loads[timeframe] ?? .idle
    }

    /// Marks the timeframe in flight and returns it when a request should start now.
    public mutating func beginFetch(for overlay: OverlayState, now: Date) -> Timeframe? {
        guard overlay.isVisible, case .expanded(let timeframe) = overlay.mode, self[timeframe].isDue(timeframe, now: now) else {
            return nil
        }
        loads[timeframe, default: .idle].inFlight = true
        return timeframe
    }

    public mutating func finish(_ timeframe: Timeframe, _ result: Result<[Candle], FetchFailure>, at now: Date) {
        guard self[timeframe].inFlight else { return }
        var load = self[timeframe]
        load.inFlight = false
        switch result {
        case .success(let candles):
            load.series = CandleSeries(candles: candles, fetchedAt: now)
            load.failure = nil
        case .failure(let kind):
            load.failure = FailedFetch(kind: kind, at: now)
        }
        loads[timeframe] = load
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

    /// Shows the newest candles the zoom asks for, with the price domain fitted to those alone.
    public init?(candles all: [Candle], timeframe: Timeframe, zoom: Zoom) {
        let candles = all.suffix(zoom.visibleCount(timeframe, available: all.count))
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

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

/// What the chart view draws, in prices and times; the view maps them onto its plot.
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
    /// How many candles the bars stand for, which is more than `bars.count` once runs of candles merge.
    public let candleCount: Int

    /// Shows the newest candles the zoom asks for, with the price domain fitted to those alone. Past `maxBars` (at least 1),
    /// each bar merges a run of adjacent candles counted from the newest, and a partial run at the oldest end is left out.
    public init?(candles all: [Candle], timeframe: Timeframe, zoom: Zoom, maxBars: Int) {
        let visible = zoom.visibleCount(timeframe, available: all.count)
        let perBar = (visible + maxBars - 1) / maxBars
        guard perBar > 0 else { return nil }
        let candles = all.suffix(visible - visible % perBar)
        let merged = stride(from: candles.startIndex, to: candles.endIndex, by: perBar).map { start in
            let run = candles[start..<start + perBar]
            return Candle(
                time: run.first!.time,
                open: run.first!.open,
                high: run.lazy.map(\.high).max()!,
                low: run.lazy.map(\.low).min()!,
                close: run.last!.close
            )
        }
        let low = merged.lazy.map(\.low).min()!
        let high = merged.lazy.map(\.high).max()!
        let range = high > low ? high - low : high * 0.001
        priceDomain = (low - range * Self.pricePadding)...(high + range * Self.pricePadding)
        let interval = TimeInterval(timeframe.spec.krakenIntervalMinutes * 60 * perBar)
        timeDomain = merged.first!.time...merged.last!.time.addingTimeInterval(interval)
        lastPrice = merged.last!.close
        candleCount = candles.count

        let minimumBody = (priceDomain.upperBound - priceDomain.lowerBound) * Self.minimumBody
        bars = merged.map { candle in
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

extension CandleChart {
    /// The fewest gridlines an axis gets; the next coarser step would leave fewer, so an axis gets 3 to 8, mostly 3 to 5.
    public static let minimumTicks = 3.0
    static let timeSteps: [TimeInterval] = [1, 2, 5, 10, 15, 30, 60, 120, 180, 360, 720, 1440, 2880, 5760, 10080, 20160, 43200].map { $0 * 60 }

    /// Multiples of a 1, 2 or 5 step inside the price domain.
    public var priceTicks: [Double] {
        let span = priceDomain.upperBound - priceDomain.lowerBound
        guard span > 0 else { return [] }
        let magnitude = pow(10, log10(span / Self.minimumTicks).rounded(.down))
        return Self.ticks(in: priceDomain, steps: [1, 2, 5].map { $0 * magnitude })
    }

    /// Round local times inside the time domain, `utcOffset` seconds east of UTC.
    public func timeTicks(utcOffset: Int) -> [Date] {
        let range = timeDomain.lowerBound.timeIntervalSince1970...timeDomain.upperBound.timeIntervalSince1970
        return Self.ticks(in: range, steps: Self.timeSteps, offset: Double(utcOffset)).map(Date.init(timeIntervalSince1970:))
    }

    private static func ticks(in range: ClosedRange<Double>, steps: [Double], offset: Double = 0) -> [Double] {
        let span = range.upperBound - range.lowerBound
        guard span > 0, let step = steps.last(where: { span / $0 >= minimumTicks }) ?? steps.first else { return [] }
        let first = ((range.lowerBound + offset) / step).rounded(.up) * step - offset
        return Array(stride(from: first, through: range.upperBound, by: step))
    }
}

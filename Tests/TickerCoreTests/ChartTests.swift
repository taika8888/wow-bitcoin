import CoreGraphics
import Foundation
import Testing
import TickerCore

private let t0 = Date(timeIntervalSince1970: 1_793_847_300)

private func candle(_ open: Double, _ close: Double, high: Double? = nil, low: Double? = nil, at minute: Int = 0) -> Candle {
    Candle(time: t0.addingTimeInterval(Double(minute) * 60), open: open, high: high ?? max(open, close), low: low ?? min(open, close), close: close)
}

/// Values read straight from the fixture JSON, not from the parser.
private let recorded: [(file: String, timeframe: Timeframe, low: Double, high: Double, up: Int, down: Int)] = [
    ("ohlc_1", .m1, 80190.1, 80827.6, 62, 57),
    ("ohlc_15", .m15, 75625.0, 81179.6, 49, 47),
    ("ohlc_240", .h4, 75625.0, 84840.0, 42, 48),
]

@Suite struct CandleChartTests {
    @Test(arguments: recorded.indices)
    func recordedCandlesFitThePaddedDomain(index: Int) throws {
        let fixtureCase = recorded[index]
        let candles = try Kraken.parseCandles(try fixture(fixtureCase.file))
        let chart = try #require(CandleChart(candles: candles, timeframe: fixtureCase.timeframe, zoom: .standard))
        let range = fixtureCase.high - fixtureCase.low
        let below = (fixtureCase.low - chart.priceDomain.lowerBound) / range
        let above = (chart.priceDomain.upperBound - fixtureCase.high) / range
        #expect((0.05...0.1).contains(below), "margin under the lowest wick is \(below) of the range")
        #expect((0.05...0.1).contains(above), "margin over the highest wick is \(above) of the range")
        #expect(chart.bars.count == fixtureCase.timeframe.spec.candlesShown)
        #expect(chart.bars.filter { $0.tone == .up }.count == fixtureCase.up)
        #expect(chart.bars.filter { $0.tone == .down }.count == fixtureCase.down)
        #expect(chart.lastPrice == 80489.7)
        for bar in chart.bars {
            #expect(chart.timeDomain.contains(bar.time))
            #expect(chart.priceDomain.contains(bar.bodyLow) && chart.priceDomain.contains(bar.bodyHigh))
        }
    }

    @Test func toneFollowsCloseAgainstOpen() throws {
        let chart = try #require(CandleChart(candles: [candle(10, 11, at: 0), candle(11, 10, at: 1), candle(10, 10, at: 2)], timeframe: .m1, zoom: .standard))
        #expect(chart.bars.map(\.tone) == [.up, .down, .neutral])
        #expect(chart.bars[0].bodyLow == 10 && chart.bars[0].bodyHigh == 11)
        #expect(chart.bars[1].bodyLow == 10 && chart.bars[1].bodyHigh == 11)
        #expect(chart.lastPrice == 10)
    }

    @Test func aFlatCandleStillHasAVisibleBody() throws {
        let chart = try #require(CandleChart(candles: [candle(100, 100, high: 120, low: 80)], timeframe: .m1, zoom: .standard))
        let bar = chart.bars[0]
        #expect(bar.bodyHigh > bar.bodyLow)
        #expect(bar.bodyLow < 100 && bar.bodyHigh > 100)
    }

    @Test func aSeriesWithNoRangeStillHasADomain() throws {
        let chart = try #require(CandleChart(candles: [candle(80000, 80000), candle(80000, 80000, at: 1)], timeframe: .m1, zoom: .standard))
        #expect(chart.priceDomain.lowerBound < 80000 && chart.priceDomain.upperBound > 80000)
    }

    @Test func barsSitMidIntervalAndTheTimeDomainCoversTheLastInterval() throws {
        let candles = [candle(1, 2, at: 0), candle(2, 3, at: 15)]
        let chart = try #require(CandleChart(candles: candles, timeframe: .m15, zoom: .standard))
        #expect(chart.bars.map(\.time) == [t0.addingTimeInterval(450), t0.addingTimeInterval(15 * 60 + 450)])
        #expect(chart.timeDomain == t0...t0.addingTimeInterval(30 * 60))
    }

    @Test func noCandlesNoChart() {
        #expect(CandleChart(candles: [], timeframe: .h4, zoom: .standard) == nil)
    }
}

@Suite struct ZoomTests {
    @Test(arguments: [
        (Timeframe.m1, [30, 60, 120, 240, 480, 720]),
        (.m15, [24, 48, 96, 192, 384, 720]),
        (.h4, [22, 45, 90, 180, 360, 720]),
    ])
    func eachRungShowsAMultipleOfTheStandardViewOrEveryCandle(timeframe: Timeframe, counts: [Int]) {
        #expect(Zoom.allCases.map { $0.visibleCount(timeframe, available: 720) } == counts)
    }

    @Test func noRungShowsMoreCandlesThanThereAreOrNoneWhenThereAreSome() {
        #expect(Zoom.allCases.map { $0.visibleCount(.m1, available: 100) } == [30, 60, 100, 100, 100, 100])
        #expect(Zoom.allCases.map { $0.visibleCount(.h4, available: 1) } == [1, 1, 1, 1, 1, 1])
        #expect(Zoom.allCases.map { $0.visibleCount(.m15, available: 0) } == [0, 0, 0, 0, 0, 0])
    }

    @Test func steppingWalksTheLadderAndStopsAtItsEnds() {
        let outward = Array(sequence(first: Zoom.quarter) { $0 == .all ? nil : $0.stepped(.out) })
        #expect(outward == [.quarter, .half, .standard, .double, .quadruple, .all])
        #expect(Array(sequence(first: Zoom.all) { $0 == .quarter ? nil : $0.stepped(.in) }) == outward.reversed())
        #expect(Zoom.quarter.stepped(.in) == .quarter)
        #expect(Zoom.all.stepped(.out) == .all)
    }
}

private func overlay(_ mode: OverlayMode, visible: Bool = true) -> OverlayState {
    var settings = Settings()
    settings.onlyWhenWoWActive = !visible
    var state = OverlayState(settings: settings, frontmost: .other)
    state.mode = mode
    return state
}

@Suite struct ChartFeedTests {
    private let candles = [candle(10, 11)]

    @Test func fetchesNothingWhileCollapsedOrHidden() {
        var feed = ChartFeed()
        #expect(feed.beginFetch(for: overlay(.collapsed), now: t0) == nil)
        #expect(feed.beginFetch(for: overlay(.expanded(.m1), visible: false), now: t0) == nil)
        #expect(feed[.m1] == .idle)
    }

    @Test func neverOverlapsRequestsForOneTimeframe() {
        var feed = ChartFeed()
        #expect(feed.beginFetch(for: overlay(.expanded(.m1)), now: t0) == .m1)
        #expect(feed.beginFetch(for: overlay(.expanded(.m1)), now: t0.addingTimeInterval(600)) == nil)
        #expect(feed[.m1].inFlight && feed[.m1].series == nil)
    }

    @Test(arguments: Timeframe.allCases)
    func refetchesOnlyOnceTheRefreshIntervalPasses(timeframe: Timeframe) {
        var feed = ChartFeed()
        let refresh = TimeInterval(timeframe.spec.refreshSeconds)
        _ = feed.beginFetch(for: overlay(.expanded(timeframe)), now: t0)
        feed.finish(timeframe, .success(candles), at: t0)
        #expect(feed[timeframe].series == CandleSeries(candles: candles, fetchedAt: t0) && feed[timeframe].notice(for: timeframe) == nil)
        #expect(feed.beginFetch(for: overlay(.expanded(timeframe)), now: t0.addingTimeInterval(refresh - 1)) == nil)
        #expect(feed.beginFetch(for: overlay(.expanded(timeframe)), now: t0.addingTimeInterval(refresh)) == timeframe)
        #expect(feed[timeframe].series?.candles == candles, "a refresh keeps showing the last candles")
    }

    @Test func switchingBackRefetchesOnlyWhenStale() {
        var feed = ChartFeed()
        _ = feed.beginFetch(for: overlay(.expanded(.m1)), now: t0)
        feed.finish(.m1, .success(candles), at: t0)
        #expect(feed.beginFetch(for: overlay(.expanded(.h4)), now: t0.addingTimeInterval(1)) == .h4)
        feed.finish(.h4, .success(candles), at: t0.addingTimeInterval(2))
        #expect(feed.beginFetch(for: overlay(.expanded(.m1)), now: t0.addingTimeInterval(5)) == nil)
        #expect(feed.beginFetch(for: overlay(.expanded(.m1)), now: t0.addingTimeInterval(20)) == .m1)
    }

    @Test func aFailureKeepsTheLastCandlesAndRetries() {
        var feed = ChartFeed()
        _ = feed.beginFetch(for: overlay(.expanded(.h4)), now: t0)
        feed.finish(.h4, .success(candles), at: t0)
        let later = t0.addingTimeInterval(300)
        _ = feed.beginFetch(for: overlay(.expanded(.h4)), now: later)
        feed.finish(.h4, .failure(.unreachable), at: later)
        #expect(feed[.h4].name == "failed")
        #expect(feed[.h4].series?.candles == candles)
        #expect(feed.beginFetch(for: overlay(.expanded(.h4)), now: later.addingTimeInterval(CandleLoad.retryAfterFailure - 1)) == nil)
        #expect(feed.beginFetch(for: overlay(.expanded(.h4)), now: later.addingTimeInterval(CandleLoad.retryAfterFailure)) == .h4)
    }

    @Test func aCompletionWithNoRequestInFlightIsIgnored() {
        var feed = ChartFeed()
        feed.finish(.m15, .success(candles), at: t0)
        #expect(feed[.m15] == .idle)
    }

    @Test func oldCandlesAreMarkedStaleFromAFailedRefreshUntilOneSucceeds() {
        var feed = ChartFeed()
        _ = feed.beginFetch(for: overlay(.expanded(.m1)), now: t0)
        feed.finish(.m1, .success(candles), at: t0)
        #expect(feed[.m1].notice(for: .m1) == nil)
        let refresh = t0.addingTimeInterval(60)
        _ = feed.beginFetch(for: overlay(.expanded(.m1)), now: refresh)
        feed.finish(.m1, .failure(.unreachable), at: refresh)
        #expect(feed[.m1].notice(for: .m1) == .stale("Stale: Kraken is not answering"))
        _ = feed.beginFetch(for: overlay(.expanded(.m1)), now: refresh.addingTimeInterval(CandleLoad.retryAfterFailure))
        #expect(feed[.m1].notice(for: .m1) == .stale("Stale: Kraken is not answering"), "still stale while the retry is out")
        feed.finish(.m1, .success(candles), at: refresh.addingTimeInterval(11))
        #expect(feed[.m1].notice(for: .m1) == nil)
    }

    @Test(arguments: [
        (FetchFailure.unreachable, "Kraken is not answering"),
        (.status(503), "Kraken answered HTTP 503"),
        (.rejected(.malformed("OHLC row 3")), "Kraken sent candles this app cannot read"),
        (.rejected(.invalidValue("row 3 high/low do not bound open/close")), "Kraken sent candles this app cannot read"),
        (.rejected(.api(["EGeneral:Too many requests"])), "Kraken refused: EGeneral:Too many requests"),
    ])
    func theMessageNamesWhatWentWrong(failure: FetchFailure, message: String) {
        var feed = ChartFeed()
        _ = feed.beginFetch(for: overlay(.expanded(.h4)), now: t0)
        feed.finish(.h4, .failure(failure), at: t0)
        #expect(feed[.h4].notice(for: .h4) == .placeholder("\(message), retrying"))
    }

    @Test func withNothingToDrawThePlaceholderSaysWhy() {
        var feed = ChartFeed()
        #expect(feed[.m15].notice(for: .m15) == .placeholder("Loading 15m candles"))
        _ = feed.beginFetch(for: overlay(.expanded(.m15)), now: t0)
        feed.finish(.m15, .success([]), at: t0)
        #expect(feed[.m15].notice(for: .m15) == .placeholder("No 15m candles"))
    }
}

@Suite struct ExpandedLayoutTests {
    private let screen = CGRect(x: 0, y: 0, width: 1728, height: 1085)
    private let segment = CGSize(width: 34, height: 19)
    private let chart = CGSize(width: 360, height: 180)

    private func layout(tickerAt origin: CGPoint) -> (ExpandedLayout, CGRect) {
        let ticker = CGRect(origin: origin, size: CGSize(width: 182, height: 27))
        return (ExpandedLayout(tickerFrame: ticker, segment: segment, chart: chart, screen: screen), ticker)
    }

    private static let corners = [CGPoint(x: 20, y: 20), CGPoint(x: 1500, y: 20), CGPoint(x: 20, y: 1040), CGPoint(x: 1530, y: 1046)]

    @Test(arguments: corners)
    func theTickerStaysPutAndTheChartOpensTowardTheMiddle(origin: CGPoint) {
        let (layout, ticker) = layout(tickerAt: origin)
        let tickerOnScreen = layout.ticker.offsetBy(dx: layout.frame.minX, dy: layout.frame.minY)
        let chartOnScreen = layout.chart.offsetBy(dx: layout.frame.minX, dy: layout.frame.minY)
        #expect(tickerOnScreen == ticker)
        #expect(screen.contains(layout.frame))
        if ticker.midY > screen.midY {
            #expect(chartOnScreen.maxY <= ticker.minY, "a ticker in the top half opens downward")
        } else {
            #expect(chartOnScreen.minY >= ticker.maxY, "a ticker in the bottom half opens upward")
        }
        if ticker.midX < screen.midX {
            #expect(layout.frame.minX == ticker.minX && layout.frame.maxX > ticker.maxX, "a ticker on the left opens rightward")
        } else {
            #expect(layout.frame.maxX == ticker.maxX && layout.frame.minX < ticker.minX, "a ticker on the right opens leftward")
        }
        #expect(layout.chart.height == chart.height && layout.chart.width >= chart.width)
    }

    @Test(arguments: corners)
    func timeframeTargetsAreDistinctAndHitTestable(origin: CGPoint) {
        let (layout, _) = layout(tickerAt: origin)
        let panel = CGRect(origin: .zero, size: layout.frame.size)
        let rects = Timeframe.allCases.map { layout.timeframes[$0]! }
        #expect(zip(rects, rects.dropFirst()).allSatisfy { $0.maxX <= $1.minX }, "1m, 15m, 4h read left to right")
        for (timeframe, rect) in zip(Timeframe.allCases, rects) {
            #expect(panel.contains(rect))
            #expect(!rect.intersects(layout.ticker) && !rect.intersects(layout.chart))
            #expect(layout.timeframe(at: CGPoint(x: rect.midX, y: rect.midY)) == timeframe)
        }
        #expect(layout.timeframe(at: CGPoint(x: layout.ticker.midX, y: layout.ticker.midY)) == nil)
        #expect(!layout.chart.intersects(layout.ticker))
    }

    @Test func aScreenTooSmallForTheChartKeepsThePanelOnScreen() {
        let small = CGRect(x: 0, y: 0, width: 400, height: 220)
        let ticker = CGRect(x: 210, y: 190, width: 182, height: 27)
        let layout = ExpandedLayout(tickerFrame: ticker, segment: segment, chart: chart, screen: small)
        #expect(layout.frame.minX >= small.minX && layout.frame.minY >= small.minY)
    }
}

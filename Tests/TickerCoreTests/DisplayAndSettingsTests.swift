import Foundation
import Testing
import TickerCore

private let fetchedAt = Date(timeIntervalSince1970: 1_793_847_300)
private let quote = Quote(price: 80482.4, open24h: 80442, fetchedAt: fetchedAt)

@Suite struct FormattingTests {
    @Test(arguments: [
        (80482.4, 0, "$80,482"),
        (80482.456, 2, "$80,482.46"),
        (80482.45, 1, "$80,482.5"),
        (999.5, 0, "$1,000"),
        (1_234_567.891, 1, "$1,234,567.9"),
        (42.0, 2, "$42.00"),
    ])
    func price(value: Double, decimals: Int, expected: String) {
        #expect(TickerDisplay.price(value, decimals: decimals) == expected)
    }

    @Test(arguments: [
        (0.0005, "+0.05%", Tone.up),
        (-0.0123, "-1.23%", .down),
        (0.12345, "+12.35%", .up),
        (0.0, "0.00%", .neutral),
        (0.00004, "0.00%", .neutral),
        (-0.00004, "0.00%", .neutral),
    ])
    func change(fraction: Double, text: String, tone: Tone) {
        #expect(TickerDisplay.change(fraction) == (text, tone))
    }

    @Test func liveTickerText() {
        let display = TickerDisplay(status: .live(quote), settings: Settings())
        #expect(display.text == "BTC $80,482  +0.05%")
        #expect(display.segments.last == DisplaySegment("+0.05%", .up))
        #expect(!display.dimmed)
    }

    @Test func labelAndChangeAreOptional() {
        var settings = Settings()
        settings.showLabel = false
        settings.decimals = 2
        #expect(TickerDisplay(status: .live(quote), settings: settings).text == "$80,482.40  +0.05%")
        settings.showLabel = true
        settings.showChange = false
        #expect(TickerDisplay(status: .live(quote), settings: settings).text == "BTC $80,482.40")
    }

    @Test func flatChangeIsNeutral() {
        let flat = Quote(price: 80442.01, open24h: 80442, fetchedAt: fetchedAt)
        let display = TickerDisplay(status: .live(flat), settings: Settings())
        #expect(display.text == "BTC $80,442  0.00%")
        #expect(display.segments.allSatisfy { $0.tone == .neutral })
    }

    @Test func notLiveIsDimmedAndLabelled() {
        let stale = TickerDisplay(status: .stale(quote), settings: Settings())
        #expect(stale.text == "BTC $80,482  +0.05%  stale")
        #expect(stale.dimmed)
        let offline = TickerDisplay(status: .offline, settings: Settings())
        #expect(offline.text == "BTC offline")
        #expect(offline.dimmed)
        let loading = TickerDisplay(status: .loading, settings: Settings())
        #expect(loading.text == "BTC ...")
        #expect(loading.dimmed)
    }
}

@Suite struct SettingsTests {
    @Test(arguments: [nil, "", "not json", "[1,2]", "null", "{}", #"{"fontSize":"#])
    func unreadableOrEmptyBlobsGiveDefaults(json: String?) {
        #expect(Settings.decode(json: json) == Settings())
    }

    @Test func missingKeysTakeDefaults() {
        var expected = Settings()
        expected.fontSize = 30
        expected.locked = true
        #expect(Settings.decode(json: #"{"fontSize":30,"locked":true,"someFutureKey":1}"#) == expected)
    }

    @Test func mistypedKeysTakeDefaultsWithoutLosingTheRest() {
        let settings = Settings.decode(json: #"{"fontSize":"big","decimals":1.5,"textColor":{"red":1},"showLabel":false}"#)
        #expect(settings.fontSize == Settings().fontSize)
        #expect(settings.decimals == Settings().decimals)
        #expect(settings.textColor == Settings().textColor)
        #expect(settings.showLabel == false)
    }

    @Test func outOfRangeValuesAreClamped() {
        let settings = Settings.decode(json: """
            {"fontSize":500,"opacity":-1,"decimals":7,"refreshSeconds":1,
             "upColor":{"red":2,"green":-1,"blue":0.5,"alpha":9}}
            """)
        #expect(settings.fontSize == 72)
        #expect(settings.opacity == 0)
        #expect(settings.decimals == 2)
        #expect(settings.refreshSeconds == 2)
        #expect(settings.upColor == RGBA(red: 1, green: 0, blue: 0.5, alpha: 1))
        #expect(Settings.decode(json: #"{"fontSize":1,"refreshSeconds":100000}"#).fontSize == 9)
        #expect(Settings.decode(json: #"{"refreshSeconds":100000}"#).refreshSeconds == 300)
    }

    @Test func chartSizeIsClampedToItsRanges() {
        let small = Settings.decode(json: #"{"chartWidth":10,"chartHeight":-5}"#)
        #expect(small.chartWidth == 240)
        #expect(small.chartHeight == 120)
        let large = Settings.decode(json: #"{"chartWidth":5000,"chartHeight":5000}"#)
        #expect(large.chartWidth == 900)
        #expect(large.chartHeight == 600)
    }

    @Test func lastChosenTimeframeLoadsAndAnUnknownOneFallsBack() {
        #expect(Settings.decode(json: #"{"timeframe":"h4"}"#).timeframe == .h4)
        #expect(Settings.decode(json: #"{"timeframe":"1w","fontSize":20}"#).timeframe == .m15)
        #expect(Settings.decode(json: #"{"timeframe":"1w","fontSize":20}"#).fontSize == 20)
    }

    @Test func defaultsMatchTheProductSpec() {
        let settings = Settings()
        #expect(settings.refreshSeconds == 5)
        #expect(settings.onlyWhenWoWActive == false)
        #expect(settings.locked == false)
        #expect(settings.position == nil)
        #expect(settings.timeframe == .m15)
    }

    @Test func roundTrips() {
        var settings = Settings()
        settings.position = ScreenPoint(x: 120, y: 840)
        settings.downColor = RGBA(red: 0.9, green: 0.1, blue: 0.2, alpha: 0.8)
        settings.onlyWhenWoWActive = true
        settings.timeframe = .m1
        settings.chartWidth = 500
        settings.chartHeight = 300
        #expect(Settings.decode(json: settings.json) == settings)
    }
}

@Suite struct FeedStatusTests {
    let start = Date(timeIntervalSince1970: 1_000_000)

    @Test func withoutAQuoteItIsLoadingThenOffline() {
        #expect(FeedStatus.derive(lastGood: nil, startedAt: start, now: start, refreshInterval: 5) == .loading)
        #expect(FeedStatus.derive(lastGood: nil, startedAt: start, now: start + 15, refreshInterval: 5) == .loading)
        #expect(FeedStatus.derive(lastGood: nil, startedAt: start, now: start + 15.001, refreshInterval: 5) == .offline)
    }

    @Test func aQuoteIsLiveUntilThreeRefreshesAreMissed() {
        let quote = Quote(price: 1, open24h: 1, fetchedAt: start + 100)
        #expect(FeedStatus.derive(lastGood: quote, startedAt: start, now: start + 100, refreshInterval: 5) == .live(quote))
        #expect(FeedStatus.derive(lastGood: quote, startedAt: start, now: start + 115, refreshInterval: 5) == .live(quote))
        #expect(FeedStatus.derive(lastGood: quote, startedAt: start, now: start + 115.001, refreshInterval: 5) == .stale(quote))
        #expect(FeedStatus.derive(lastGood: quote, startedAt: start, now: start + 130, refreshInterval: 10) == .live(quote))
    }

    @Test func backoffDoublesPerFailureAndCapsAtAMinute() {
        #expect((0...4).map { FeedStatus.nextFetchDelay(refreshSeconds: 5, consecutiveFailures: $0) } == [5, 10, 20, 40, 60])
        #expect(FeedStatus.nextFetchDelay(refreshSeconds: 5, consecutiveFailures: 50) == 60)
        #expect(FeedStatus.nextFetchDelay(refreshSeconds: 120, consecutiveFailures: 1) == 120)
    }
}

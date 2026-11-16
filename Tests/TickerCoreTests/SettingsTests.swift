import Foundation
import Testing
import TickerCore

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

    @Test func lastZoomLoadsAndAnUnknownOrMissingOneIsTheStandardView() {
        #expect(Settings.decode(json: #"{"zoom":5}"#).zoom == .all)
        #expect(Settings.decode(json: #"{"zoom":9,"fontSize":20}"#).zoom == .standard)
        #expect(Settings.decode(json: #"{"zoom":"all"}"#).zoom == .standard)
        #expect(Settings.decode(json: #"{"timeframe":"h4"}"#).zoom == .standard)
    }

    @Test func defaultsMatchTheProductSpec() {
        let settings = Settings()
        #expect(settings.refreshSeconds == 5)
        #expect(settings.onlyWhenWoWActive == false)
        #expect(settings.locked == false)
        #expect(settings.position == nil)
        #expect(settings.timeframe == .m15)
        #expect(settings.zoom == .standard)
    }

    @Test func roundTrips() {
        var settings = Settings()
        settings.position = ScreenPoint(x: 120, y: 840)
        settings.downColor = RGBA(red: 0.9, green: 0.1, blue: 0.2, alpha: 0.8)
        settings.onlyWhenWoWActive = true
        settings.timeframe = .m1
        settings.zoom = .quarter
        settings.chartWidth = 500
        settings.chartHeight = 300
        #expect(Settings.decode(json: settings.json) == settings)
    }
}

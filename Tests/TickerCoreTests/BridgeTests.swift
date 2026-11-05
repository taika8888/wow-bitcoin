import Foundation
import Testing
import TickerCore

private func fixture(_ name: String) throws -> String {
    let url = try #require(Bundle.module.url(forResource: name, withExtension: "json", subdirectory: "Fixtures/kraken"))
    return try String(contentsOf: url, encoding: .utf8)
}

private func fetches(_ reply: CoreReply) -> [CoreEffect.Fetch] {
    reply.effects.compactMap { if case .fetch(let fetch) = $0 { fetch } else { nil } }
}

@Suite struct BridgeTests {
    @Test func anExistingUserDefaultsBlobLoadsAndRoundTripsIdentically() throws {
        let json = #"{"fontSize":23,"opacity":0.4,"textColor":{"red":0.8,"green":0.7,"blue":0.6,"alpha":0.5},"upColor":{"red":0.1,"green":0.9,"blue":0.2,"alpha":1},"downColor":{"red":0.9,"green":0.1,"blue":0.2,"alpha":0.8},"decimals":2,"showLabel":false,"showChange":false,"locked":true,"refreshSeconds":17,"onlyWhenWoWActive":true,"position":{"x":-120,"y":840},"timeframe":"h4","zoom":4,"chartWidth":500,"chartHeight":300}"#
        let old = try JSONDecoder().decode(Settings.self, from: Data(json.utf8))
        let app = try RustApp(settingsJSON: json, frontmost: .wow, now: 1000)
        let view = try app.view()
        #expect(view.settings == old)
        let saved = try JSONSerialization.jsonObject(with: Data(view.settings.json.utf8)) as? NSDictionary
        #expect(saved == (try JSONSerialization.jsonObject(with: Data(json.utf8)) as? NSDictionary))
        let reloaded = try RustApp(settingsJSON: view.settings.json, frontmost: .other, now: 1000)
        #expect(try reloaded.view().settings == old)
        #expect(try !reloaded.view().overlayState.isVisible)
    }

    @Test func integralJSONNumbersKeepSwiftsSettingsMeaning() {
        let settings = Settings.decode(json: #"{"decimals":1.0,"zoom":5.0,"refreshSeconds":2e1}"#)
        #expect(settings.decimals == 1 && settings.zoom == .all && settings.refreshSeconds == 20)
        #expect(Settings.decode(json: #"{"refreshSeconds":9223372036854775808}"#).refreshSeconds == 5)
    }

    @Test func scriptDrivesRequestsRenderingSavesAndLateReplies() throws {
        let app = try RustApp(settingsJSON: "{}", frontmost: .wow, now: 1000)
        #expect(try app.view().display.text == "BTC ...")
        let first = try app.send(["type": "tick", "now": 1000])
        let quote = try #require(fetches(first).first)
        #expect(quote.timeframe == nil && quote.timeout == 5)
        #expect(quote.url.absoluteString == "https://api.kraken.com/0/public/Ticker?pair=XBTUSD")
        #expect(try fetches(app.send(["type": "tick", "now": 1001])).isEmpty)
        let result: [String: Any] = ["type": "quote_finished", "request": quote.request, "now": 1001,
                                   "outcome": ["type": "answered", "status": 200, "body": #"{"result":{"XXBTZUSD":{"c":["80482.4"],"o":"80442"}}}"#]]
        let loaded = try app.send(result)
        #expect(loaded.effects.count == 1)
        #expect(loaded.nextWake == 1005)
        #expect(try app.view().display.text == "BTC $80,482  +0.05%")
        #expect(try app.view().status.fetchedAt == Date(timeIntervalSince1970: 1001))
        #expect(try app.send(result).effects.isEmpty)
        let lock = try app.send(OverlayEvent.toggleLock.input)
        #expect(lock.effects.count == 2)
        if case .saveSettings(let saved) = lock.effects.first { #expect(saved.locked) } else { Issue.record("missing save") }
        let external = try app.send(["type": "settings_changed", "settings": ["locked": false, "refreshSeconds": 2]])
        #expect(!external.effects.contains { if case .saveSettings = $0 { true } else { false } })
        #expect(external.nextWake == 1002)
        #expect(try !app.view().settings.locked)
        #expect(try fetches(app.send(["type": "woke", "now": 1010])).count == 1)
    }

    @Test(arguments: [(Timeframe.m1, "ohlc_1", 120), (.m15, "ohlc_15", 96), (.h4, "ohlc_240", 90)])
    func chartAndTicksDecodeFromTheRealBridge(timeframe: Timeframe, file: String, count: Int) throws {
        let app = try RustApp(settingsJSON: "{\"timeframe\":\"\(timeframe.rawValue)\"}", frontmost: .wow, now: 1000)
        _ = try app.send(OverlayEvent.pressed.input)
        let open = try app.send(OverlayEvent.released(over: .body, tickerOrigin: ScreenPoint(x: 0, y: 0)).input)
        let request = try #require(fetches(open).first { $0.timeframe == timeframe })
        #expect(request.timeout == 10)
        _ = try app.send(["type": "candles_finished", "request": request.request, "timeframe": timeframe.rawValue,
                          "now": 1001, "outcome": ["type": "answered", "status": 200, "body": try fixture(file)]])
        let metadata = try app.view()
        #expect(metadata.chart == nil && metadata.chartRevision == 1)
        #expect(metadata.load?.name == "loaded" && metadata.load?.notice == nil)
        let view = try app.view(maxBars: 150, utcOffset: -18000)
        let chart = try #require(view.chart)
        #expect(chart.candleCount == count && chart.bars.count == count)
        #expect(chart.lastPrice == 80489.7)
        #expect((3...8).contains(chart.priceTicks.count) && (3...8).contains(chart.timeTicks.count))
        #expect(chart.priceTicks.allSatisfy { chart.priceDomain.contains($0.value) && $0.label.hasPrefix("$") })
        #expect(chart.timeTicks.allSatisfy { chart.timeDomain.contains($0) })
        _ = try app.send(OverlayEvent.zoom(.out).input)
        #expect(try app.view().chartRevision == 1, "zoom does not invent new candle data")
        #expect(try app.view(maxBars: 150).chart!.candleCount > count)
    }

    @Test func badInputsReturnErrorsAndLeaveTheAppUsable() throws {
        let app = try RustApp(settingsJSON: "{}", frontmost: .other, now: 1000)
        for json in ["{", "null", #"{"type":"tick","now":"later"}"#, #"{"type":"overlay","event":{"type":"zoom","inward":null}}"#] {
            #expect(throws: CoreError.self) { try app.send(json: json) }
        }
        #expect(try app.view().display.text == "BTC ...")
        #expect(try fetches(app.send(["type": "tick", "now": 1000])).count == 1)
    }

    @Test func failureNoticeAndRetrySurviveTheBoundary() throws {
        let app = try RustApp(settingsJSON: "{}", frontmost: .wow, now: 1000)
        _ = try app.send(OverlayEvent.pressed.input)
        let open = try app.send(OverlayEvent.released(over: .body, tickerOrigin: ScreenPoint(x: 0, y: 0)).input)
        let request = try #require(fetches(open).first { $0.timeframe != nil })
        _ = try app.send(["type": "candles_finished", "request": request.request, "timeframe": "m15", "now": 1001,
                          "outcome": ["type": "answered", "status": 503, "body": "unavailable"]])
        #expect(try app.view().load?.notice?.text == "Kraken answered HTTP 503, retrying")
        let retry = try fetches(app.send(["type": "tick", "now": 1011]))
        #expect(retry.contains { $0.timeframe == .m15 })
    }
}

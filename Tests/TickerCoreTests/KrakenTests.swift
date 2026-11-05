import Foundation
import Testing
import TickerCore

func fixture(_ name: String) throws -> Data {
    let url = try #require(Bundle.module.url(forResource: name, withExtension: "json", subdirectory: "Fixtures/kraken"))
    return try Data(contentsOf: url)
}

private func tickerBody(last: String, open: String) -> Data {
    Data(#"{"error":[],"result":{"XXBTZUSD":{"c":["\#(last)","0.1"],"o":"\#(open)"}}}"#.utf8)
}

private func ohlcBody(_ rows: String) -> Data {
    Data(#"{"error":[],"result":{"XXBTZUSD":[\#(rows)],"last":1793847240}}"#.utf8)
}

private enum Failure: Equatable {
    case api, malformed, invalidValue
}

private func failure<T>(_ parse: () throws(KrakenError) -> T) -> Failure? {
    do {
        _ = try parse()
        return nil
    } catch {
        switch error {
        case .api: return .api
        case .malformed: return .malformed
        case .invalidValue: return .invalidValue
        }
    }
}

@Suite struct KrakenTickerTests {
    @Test func parsesTheRecordedTicker() throws {
        let fetchedAt = Date(timeIntervalSince1970: 1_793_847_300)
        let quote = try Kraken.parseQuote(try fixture("ticker"), fetchedAt: fetchedAt)
        #expect(quote == Quote(price: 80489.7, open24h: 80440.8, fetchedAt: fetchedAt))
        #expect(abs(quote.change24h - (80489.7 - 80440.8) / 80440.8) < 1e-12)
    }

    @Test func krakenErrorArrayIsAnAPIError() {
        let body = Data(#"{"error":["EQuery:Unknown asset pair"]}"#.utf8)
        #expect(throws: KrakenError.api(["EQuery:Unknown asset pair"])) {
            try Kraken.parseQuote(body, fetchedAt: .now)
        }
    }

    @Test(arguments: ["nan", "NaN", "inf", "-80000", "0", "-0.0"])
    func rejectsPricesThatAreNotFiniteAndPositive(last: String) {
        #expect(failure { () throws(KrakenError) in try Kraken.parseQuote(tickerBody(last: last, open: "80000"), fetchedAt: .now) } == .invalidValue)
    }

    @Test func rejectsANegativeOpen() {
        #expect(failure { () throws(KrakenError) in try Kraken.parseQuote(tickerBody(last: "80000", open: "-1"), fetchedAt: .now) } == .invalidValue)
    }

    @Test(arguments: [
        "not json",
        #"{"error":[]}"#,
        #"{"error":[],"result":{}}"#,
        #"{"error":[],"result":{"XXBTZUSD":{"c":[],"o":"1"}}}"#,
        #"{"error":[],"result":{"XXBTZUSD":{"c":["abc"],"o":"1"}}}"#,
        #"{"error":[],"result":{"XXBTZUSD":{"c":["1"],"o":"1"},"XETHZUSD":{"c":["1"],"o":"1"}}}"#,
    ])
    func rejectsBodiesThatAreNotATicker(body: String) {
        #expect(failure { () throws(KrakenError) in try Kraken.parseQuote(Data(body.utf8), fetchedAt: .now) } == .malformed)
    }
}

@Suite struct KrakenOHLCTests {
    @Test(arguments: [(Timeframe.m1, "ohlc_1"), (.m15, "ohlc_15"), (.h4, "ohlc_240")])
    func keepsTheNewest720CandlesOfTheRecorded721OldestFirst(timeframe: Timeframe, file: String) throws {
        let candles = try Kraken.parseCandles(try fixture(file))
        #expect(candles.count == 720)
        let step = TimeInterval(timeframe.spec.krakenIntervalMinutes * 60)
        for (older, newer) in zip(candles, candles.dropFirst()) {
            #expect(newer.time.timeIntervalSince(older.time) == step)
        }
        for candle in candles {
            #expect(candle.low <= min(candle.open, candle.close))
            #expect(candle.high >= max(candle.open, candle.close))
        }
    }

    @Test func newestRecordedCandleIsLast() throws {
        let candles = try Kraken.parseCandles(try fixture("ohlc_1"))
        #expect(candles.last?.time == Date(timeIntervalSince1970: 1_793_847_300))
    }

    @Test func parsesARow() throws {
        let candles = try Kraken.parseCandles(ohlcBody(#"[1793804100,"10","12","9","11","10.5","1.0",3]"#))
        #expect(candles == [Candle(time: Date(timeIntervalSince1970: 1_793_804_100), open: 10, high: 12, low: 9, close: 11)])
    }

    @Test(arguments: [
        #"[1,"10","9","12","11","10","1",1]"#,
        #"[1,"10","10.5","9","11","10","1",1]"#,
        #"[1,"10","12","10.5","11","10","1",1]"#,
        #"[1,"10","12","9","nan","10","1",1]"#,
        #"[1,"10","12","-9","11","10","1",1]"#,
    ])
    func rejectsCandlesWhoseRangeDoesNotHold(row: String) {
        #expect(failure { () throws(KrakenError) in try Kraken.parseCandles(ohlcBody(row)) } == .invalidValue)
    }

    @Test func rejectsShortRows() {
        #expect(failure { () throws(KrakenError) in try Kraken.parseCandles(ohlcBody(#"[1,"10","12"]"#)) } == .malformed)
    }

    @Test func krakenErrorArrayIsAnAPIError() {
        let body = Data(#"{"error":["EGeneral:Too many requests"],"result":{}}"#.utf8)
        #expect(failure { () throws(KrakenError) in try Kraken.parseCandles(body) } == .api)
    }

    @Test func timeframeRegistry() {
        #expect(Timeframe.allCases.map(\.spec.krakenIntervalMinutes) == [1, 15, 240])
        #expect(Timeframe.allCases.map(\.spec.candlesShown) == [120, 96, 90])
        #expect(Timeframe.allCases.map(\.spec.refreshSeconds) == [15, 60, 300])
        #expect(Kraken.ohlcURL(.h4).absoluteString == "https://api.kraken.com/0/public/OHLC?pair=XBTUSD&interval=240")
    }
}

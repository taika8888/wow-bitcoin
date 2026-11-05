import Foundation

public enum KrakenError: Error, Hashable, Sendable {
    /// Kraken answered with a non-empty `error` array.
    case api([String])
    /// The body is not shaped like the documented response.
    case malformed(String)
    /// A price is not finite and positive, or a candle's high and low do not bound its open and close.
    case invalidValue(String)
}

/// The only place that knows Kraken's wire format. Everything past these functions trusts the domain types.
public enum Kraken {
    /// Kraken's OHLC endpoint answers with 721 rows, the newest still forming. Keeping 720 lets the chart zoom out with no extra request.
    public static let maxCandles = 720
    public static let tickerURL = URL(string: "https://api.kraken.com/0/public/Ticker?pair=XBTUSD")!

    public static func ohlcURL(_ timeframe: Timeframe) -> URL {
        URL(string: "https://api.kraken.com/0/public/OHLC?pair=XBTUSD&interval=\(timeframe.spec.krakenIntervalMinutes)")!
    }

    public static func parseQuote(_ data: Data, fetchedAt: Date) throws(KrakenError) -> Quote {
        guard let ticker = try pairEntry(data) as? [String: Any],
              let last = ticker["c"] as? [Any], !last.isEmpty
        else { throw .malformed("ticker has no c[0]") }
        return Quote(
            price: try price(last[0], "c[0]"),
            open24h: try price(ticker["o"], "o"),
            fetchedAt: fetchedAt
        )
    }

    /// Oldest first, the newest `maxCandles`. One bad row rejects the response.
    public static func parseCandles(_ data: Data) throws(KrakenError) -> [Candle] {
        guard let rows = try pairEntry(data) as? [Any] else { throw .malformed("OHLC result is not an array") }
        var candles: [Candle] = []
        candles.reserveCapacity(rows.count)
        for (index, raw) in rows.enumerated() {
            guard let row = raw as? [Any], row.count >= 5, let time = row[0] as? NSNumber else {
                throw .malformed("OHLC row \(index)")
            }
            let candle = Candle(
                time: Date(timeIntervalSince1970: time.doubleValue),
                open: try price(row[1], "row \(index) open"),
                high: try price(row[2], "row \(index) high"),
                low: try price(row[3], "row \(index) low"),
                close: try price(row[4], "row \(index) close")
            )
            guard candle.low <= min(candle.open, candle.close), candle.high >= max(candle.open, candle.close) else {
                throw .invalidValue("row \(index) high/low do not bound open/close")
            }
            candles.append(candle)
        }
        return Array(candles.suffix(maxCandles))
    }

    private static func pairEntry(_ data: Data) throws(KrakenError) -> Any {
        guard let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
            throw .malformed("body is not a JSON object")
        }
        if let errors = root["error"] as? [String], !errors.isEmpty { throw .api(errors) }
        guard let result = root["result"] as? [String: Any] else { throw .malformed("no result object") }
        let pairs = result.filter { $0.key != "last" }
        guard pairs.count == 1, let entry = pairs.first?.value else {
            throw .malformed("expected one pair, got \(pairs.keys.sorted())")
        }
        return entry
    }

    private static func price(_ raw: Any?, _ field: String) throws(KrakenError) -> Double {
        let value: Double? = switch raw {
        case let text as String: Double(text)
        case let number as NSNumber: number.doubleValue
        default: nil
        }
        guard let value else { throw .malformed("\(field) is not a number") }
        guard value.isFinite, value > 0 else { throw .invalidValue("\(field) = \(value)") }
        return value
    }
}

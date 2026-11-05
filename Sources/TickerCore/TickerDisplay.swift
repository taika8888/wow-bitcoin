import Foundation

public enum Tone: String, Hashable, Sendable {
    case neutral, up, down
}

public struct DisplaySegment: Hashable, Sendable {
    public let text: String
    public let tone: Tone

    public init(_ text: String, _ tone: Tone = .neutral) {
        self.text = text
        self.tone = tone
    }
}

public struct TickerDisplay: Hashable, Sendable {
    public let segments: [DisplaySegment]
    public let dimmed: Bool

    public var text: String { segments.map(\.text).joined() }

    public init(status: FeedStatus, settings: Settings) {
        let label = settings.showLabel ? "BTC " : ""
        switch status {
        case .loading:
            segments = [DisplaySegment(label + "...")]
            dimmed = true
        case .offline:
            segments = [DisplaySegment(label + "offline")]
            dimmed = true
        case .live(let quote), .stale(let quote):
            let isStale = if case .stale = status { true } else { false }
            var runs = [DisplaySegment(label + Self.price(quote.price, decimals: settings.decimals))]
            if settings.showChange {
                let change = Self.change(quote.change24h)
                runs += [DisplaySegment("  "), DisplaySegment(change.text, change.tone)]
            }
            if isStale { runs.append(DisplaySegment("  stale")) }
            segments = runs
            dimmed = isStale
        }
    }

    public static func price(_ value: Double, decimals: Int) -> String {
        "$" + value.formatted(number.precision(.fractionLength(decimals)).grouping(.automatic))
    }

    /// Rounds to the shown precision first, so a change that displays as 0.00% is flat, never "+0.00%" in green.
    public static func change(_ fraction: Double) -> (text: String, tone: Tone) {
        let percent = (fraction * 10_000).rounded() / 100
        let magnitude = abs(percent).formatted(number.precision(.fractionLength(2)).grouping(.never)) + "%"
        if percent > 0 { return ("+" + magnitude, .up) }
        if percent < 0 { return ("-" + magnitude, .down) }
        return (magnitude, .neutral)
    }

    private static let number = FloatingPointFormatStyle<Double>(locale: Locale(identifier: "en_US"))
        .rounded(rule: .toNearestOrAwayFromZero)
}

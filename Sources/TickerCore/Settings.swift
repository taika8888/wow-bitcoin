import Foundation

public struct RGBA: Codable, Hashable, Sendable {
    public var red: Double
    public var green: Double
    public var blue: Double
    public var alpha: Double

    public init(red: Double, green: Double, blue: Double, alpha: Double = 1) {
        self.red = red
        self.green = green
        self.blue = blue
        self.alpha = alpha
    }

    fileprivate func clamped() -> RGBA {
        RGBA(red: red.clamped(to: 0...1), green: green.clamped(to: 0...1), blue: blue.clamped(to: 0...1), alpha: alpha.clamped(to: 0...1))
    }
}

/// A window origin in AppKit screen coordinates (bottom-left origin).
public struct ScreenPoint: Codable, Hashable, Sendable {
    public var x: Double
    public var y: Double

    public init(x: Double, y: Double) {
        self.x = x
        self.y = y
    }
}

public struct Settings: Codable, Hashable, Sendable {
    public static let fontSizeRange: ClosedRange<Double> = 9...72
    public static let opacityRange: ClosedRange<Double> = 0...1
    public static let decimalsRange: ClosedRange<Int> = 0...2
    public static let refreshSecondsRange: ClosedRange<Int> = 2...300
    public static let chartWidthRange: ClosedRange<Double> = 240...900
    public static let chartHeightRange: ClosedRange<Double> = 120...600

    public var fontSize: Double = 15
    public var opacity: Double = 0.55
    public var textColor = RGBA(red: 1, green: 1, blue: 1)
    public var upColor = RGBA(red: 0.33, green: 0.86, blue: 0.47)
    public var downColor = RGBA(red: 1, green: 0.42, blue: 0.42)
    public var decimals = 0
    public var showLabel = true
    public var showChange = true
    public var locked = false
    public var refreshSeconds = 5
    public var onlyWhenWoWActive = false
    /// nil places the overlay at the default spot at the top center of the main screen.
    public var position: ScreenPoint?
    /// The timeframe the chart opens on, which is the one last chosen.
    public var timeframe = Timeframe.m15
    public var chartWidth: Double = 360
    public var chartHeight: Double = 180

    public init() {}

    /// Missing, mistyped, or out-of-range keys fall back to the default or the nearest bound, so an old blob still loads.
    public init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        func value<T: Decodable>(_ key: CodingKeys, _ fallback: T) -> T {
            ((try? container.decodeIfPresent(T.self, forKey: key)) ?? nil) ?? fallback
        }
        let defaults = Settings()
        fontSize = value(.fontSize, defaults.fontSize).clamped(to: Self.fontSizeRange)
        opacity = value(.opacity, defaults.opacity).clamped(to: Self.opacityRange)
        textColor = value(.textColor, defaults.textColor).clamped()
        upColor = value(.upColor, defaults.upColor).clamped()
        downColor = value(.downColor, defaults.downColor).clamped()
        decimals = value(.decimals, defaults.decimals).clamped(to: Self.decimalsRange)
        showLabel = value(.showLabel, defaults.showLabel)
        showChange = value(.showChange, defaults.showChange)
        locked = value(.locked, defaults.locked)
        refreshSeconds = value(.refreshSeconds, defaults.refreshSeconds).clamped(to: Self.refreshSecondsRange)
        onlyWhenWoWActive = value(.onlyWhenWoWActive, defaults.onlyWhenWoWActive)
        position = value(.position, defaults.position)
        timeframe = value(.timeframe, defaults.timeframe)
        chartWidth = value(.chartWidth, defaults.chartWidth).clamped(to: Self.chartWidthRange)
        chartHeight = value(.chartHeight, defaults.chartHeight).clamped(to: Self.chartHeightRange)
    }

    public static func decode(json: String?) -> Settings {
        guard let data = json?.data(using: .utf8), let settings = try? JSONDecoder().decode(Settings.self, from: data) else {
            return Settings()
        }
        return settings
    }

    public var json: String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = .sortedKeys
        return String(decoding: try! encoder.encode(self), as: UTF8.self)
    }
}

extension Comparable {
    fileprivate func clamped(to range: ClosedRange<Self>) -> Self {
        min(max(self, range.lowerBound), range.upperBound)
    }
}

import Foundation

public enum FeedStatus: Hashable, Sendable {
    case loading
    case live(Quote)
    case stale(Quote)
    case offline

    public static let missedRefreshesBeforeStale = 3.0

    /// `startedAt` separates "still waiting for the first quote" from "never got one".
    public static func derive(lastGood: Quote?, startedAt: Date, now: Date, refreshInterval: TimeInterval) -> FeedStatus {
        let threshold = refreshInterval * missedRefreshesBeforeStale
        guard let quote = lastGood else {
            return now.timeIntervalSince(startedAt) > threshold ? .offline : .loading
        }
        return now.timeIntervalSince(quote.fetchedAt) > threshold ? .stale(quote) : .live(quote)
    }

    public var name: String {
        switch self {
        case .loading: "loading"
        case .live: "live"
        case .stale: "stale"
        case .offline: "offline"
        }
    }
}

/// When the next quote request starts. Each wait runs from the previous request's start, so a slow or
/// timed-out request does not push the cadence back.
public struct FeedSchedule: Hashable, Sendable {
    public private(set) var lastStart: Date?
    public private(set) var failures = 0

    public init() {}

    /// Capped at the interval: with the cadence anchored on starts, a good quote, a failure and a good retry
    /// then land within three intervals, so one failure never reaches the stale threshold.
    public static func requestTimeout(refreshSeconds: Int) -> TimeInterval {
        min(10, TimeInterval(refreshSeconds))
    }

    public mutating func started(at time: Date) {
        lastStart = time
    }

    public mutating func finished(ok: Bool) {
        failures = ok ? 0 : failures + 1
    }

    /// Failures from before a sleep say nothing about the network after it.
    public mutating func woke() {
        failures = 0
    }

    public func nextStart(refreshSeconds: Int) -> Date {
        guard let lastStart else { return .distantPast }
        return lastStart.addingTimeInterval(Self.delay(refreshSeconds: refreshSeconds, failures: failures))
    }

    /// The first retry keeps the normal cadence; backoff starts at the second failure in a row.
    public static func delay(refreshSeconds: Int, failures: Int) -> TimeInterval {
        let base = TimeInterval(refreshSeconds)
        guard failures > 1 else { return base }
        return max(base, min(base * pow(2, Double(min(failures - 1, 6))), 60))
    }
}

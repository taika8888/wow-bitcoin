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

    public static func nextFetchDelay(refreshSeconds: Int, consecutiveFailures: Int) -> TimeInterval {
        let base = TimeInterval(refreshSeconds)
        guard consecutiveFailures > 0 else { return base }
        return max(base, min(base * pow(2, Double(min(consecutiveFailures, 6))), 60))
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

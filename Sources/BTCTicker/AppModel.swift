import AppKit
import Observation
import TickerCore

@MainActor @Observable
final class AppModel {
    private(set) var overlay: OverlayState
    private(set) var status: FeedStatus = .loading
    private(set) var charts = ChartFeed()
    @ObservationIgnored var onChange: () -> Void = {}
    @ObservationIgnored private var lastGood: Quote?
    @ObservationIgnored private var schedule = FeedSchedule()
    @ObservationIgnored private var feedWait: Task<Void, any Error>?
    @ObservationIgnored private let startedAt = Date()
    @ObservationIgnored private let store: SettingsStore
    @ObservationIgnored private let session = URLSession(configuration: .ephemeral)

    init(store: SettingsStore, frontmost: Frontmost) {
        self.store = store
        overlay = OverlayState(settings: store.load(), frontmost: frontmost)
        store.onExternalChange = { [weak self] in self?.send(.settingsEdited($0)) }
    }

    func send(_ event: OverlayEvent) {
        let next = OverlayState.reduce(overlay, event)
        guard next != overlay else { return }
        if next.settings != overlay.settings { store.save(next.settings) }
        let retimed = next.settings.refreshSeconds != overlay.settings.refreshSeconds
        overlay = next
        if retimed { rescheduleFeed() }
        status = derivedStatus()
        onChange()
        pumpCharts()
    }

    /// Sequential on purpose: the next request starts only after the previous one finished, so requests never overlap.
    func runFeed() async {
        while !Task.isCancelled {
            let refreshSeconds = overlay.settings.refreshSeconds
            schedule.started(at: .now)
            do {
                let request = URLRequest(
                    url: Kraken.tickerURL,
                    cachePolicy: .reloadIgnoringLocalCacheData,
                    timeoutInterval: FeedSchedule.requestTimeout(refreshSeconds: refreshSeconds)
                )
                let (data, response) = try await session.data(for: request)
                guard (response as? HTTPURLResponse)?.statusCode == 200 else { throw URLError(.badServerResponse) }
                lastGood = try Kraken.parseQuote(data, fetchedAt: .now)
                schedule.finished(ok: true)
            } catch {
                schedule.finished(ok: false)
                FileHandle.standardError.write(Data("quote fetch failed (\(schedule.failures) in a row): \(error)\n".utf8))
            }
            tick()
            await waitForNextFetch()
        }
    }

    /// `rescheduleFeed()` cuts the sleep short so the wait is measured again against the current settings and schedule.
    private func waitForNextFetch() async {
        while true {
            let wait = schedule.nextStart(refreshSeconds: overlay.settings.refreshSeconds).timeIntervalSinceNow
            guard wait > 0 else { return }
            let sleep = Task { try await Task.sleep(for: .seconds(wait)) }
            feedWait = sleep
            if (try? await sleep.value) != nil { return }
        }
    }

    /// The backoff and the wait were measured against a network that may have come back during sleep.
    func woke() {
        schedule.woke()
        rescheduleFeed()
    }

    private func rescheduleFeed() {
        feedWait?.cancel()
    }

    /// Re-derives the status between fetches so a quote turns stale on time.
    func runClock() async {
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(1))
            tick()
            pumpCharts()
        }
    }

    /// `beginFetch` marks the timeframe loading before the request starts, so a second call cannot overlap it.
    private func pumpCharts() {
        guard let timeframe = charts.beginFetch(for: overlay, now: .now) else { return }
        onChange()
        Task {
            let candles = await fetchCandles(timeframe)
            charts.finish(timeframe, candles: candles, at: .now)
            onChange()
        }
    }

    private func fetchCandles(_ timeframe: Timeframe) async -> [Candle]? {
        do {
            let request = URLRequest(url: Kraken.ohlcURL(timeframe), cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 10)
            let (data, response) = try await session.data(for: request)
            guard (response as? HTTPURLResponse)?.statusCode == 200 else { throw URLError(.badServerResponse) }
            return try Kraken.parseCandles(data, timeframe: timeframe)
        } catch {
            FileHandle.standardError.write(Data("\(timeframe.spec.label) candle fetch failed: \(error)\n".utf8))
            return nil
        }
    }

    private func tick() {
        let next = derivedStatus()
        guard next != status else { return }
        status = next
        onChange()
    }

    private func derivedStatus() -> FeedStatus {
        FeedStatus.derive(
            lastGood: lastGood,
            startedAt: startedAt,
            now: .now,
            refreshInterval: TimeInterval(overlay.settings.refreshSeconds)
        )
    }
}

/// Persists Settings as a JSON string and applies edits made to the defaults domain from outside the app.
@MainActor
final class SettingsStore {
    static let e2eSuite = "dev.local.btcticker.e2e"
    var onExternalChange: (Settings) -> Void = { _ in }
    private let defaults: UserDefaults
    private var observation: NSKeyValueObservation?

    /// `BTCTICKER_SETTINGS` seeds a separate suite, so a test run never reads or writes the user's real settings.
    init(environment: [String: String]) {
        if let seed = environment["BTCTICKER_SETTINGS"] {
            defaults = UserDefaults(suiteName: Self.e2eSuite)!
            defaults.btcTickerSettings = Settings.decode(json: seed).json
        } else {
            defaults = .standard
        }
        observation = defaults.observe(\.btcTickerSettings) { @Sendable [weak self] _, _ in
            Task { @MainActor in
                guard let self else { return }
                self.onExternalChange(self.load())
            }
        }
    }

    func load() -> Settings {
        Settings.decode(json: defaults.btcTickerSettings)
    }

    func save(_ settings: Settings) {
        defaults.btcTickerSettings = settings.json
    }
}

extension UserDefaults {
    /// The KVO key path must match the defaults key, hence the property name.
    @objc dynamic var btcTickerSettings: String? {
        get { string(forKey: "btcTickerSettings") }
        set { set(newValue, forKey: "btcTickerSettings") }
    }
}

extension Frontmost {
    init(_ app: NSRunningApplication?) {
        if app?.processIdentifier == ProcessInfo.processInfo.processIdentifier {
            self = .ticker
        } else if app?.bundleIdentifier == Frontmost.wowBundleID {
            self = .wow
        } else {
            self = .other
        }
    }
}

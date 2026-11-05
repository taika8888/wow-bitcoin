import AppKit
import Observation
import TickerCore

@MainActor @Observable
final class AppModel {
    private(set) var overlay: OverlayState
    private(set) var status: FeedStatus = .loading
    @ObservationIgnored var onChange: () -> Void = {}
    @ObservationIgnored private var lastGood: Quote?
    @ObservationIgnored private let startedAt = Date()
    @ObservationIgnored private let store: SettingsStore

    init(store: SettingsStore, frontmost: Frontmost) {
        self.store = store
        overlay = OverlayState(settings: store.load(), frontmost: frontmost)
        store.onExternalChange = { [weak self] in self?.send(.settingsEdited($0)) }
    }

    func send(_ event: OverlayEvent) {
        let next = OverlayState.reduce(overlay, event)
        guard next != overlay else { return }
        if next.settings != overlay.settings { store.save(next.settings) }
        overlay = next
        status = derivedStatus()
        onChange()
    }

    /// Sequential on purpose: the next request starts only after the previous one finished, so requests never overlap.
    func runFeed() async {
        let session = URLSession(configuration: .ephemeral)
        var failures = 0
        while !Task.isCancelled {
            do {
                let request = URLRequest(url: Kraken.tickerURL, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 10)
                let (data, response) = try await session.data(for: request)
                guard (response as? HTTPURLResponse)?.statusCode == 200 else { throw URLError(.badServerResponse) }
                lastGood = try Kraken.parseQuote(data, fetchedAt: .now)
                failures = 0
            } catch {
                failures += 1
                FileHandle.standardError.write(Data("quote fetch failed (\(failures) in a row): \(error)\n".utf8))
            }
            tick()
            let delay = FeedStatus.nextFetchDelay(refreshSeconds: overlay.settings.refreshSeconds, consecutiveFailures: failures)
            try? await Task.sleep(for: .seconds(delay))
        }
    }

    /// Re-derives the status between fetches so a quote turns stale on time.
    func runClock() async {
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(1))
            tick()
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

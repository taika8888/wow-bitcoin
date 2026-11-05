import AppKit
import Observation
import TickerCore

@MainActor @Observable
final class AppModel {
    private(set) var state: CoreView
    var overlay: OverlayState { state.overlayState }
    @ObservationIgnored var onChange: () -> Void = {}
    @ObservationIgnored private let core: RustApp
    @ObservationIgnored private let store: SettingsStore
    @ObservationIgnored private let session = URLSession(configuration: .ephemeral)
    @ObservationIgnored private var heartbeat: Timer?
    @ObservationIgnored private var wakeTimer: Timer?
    @ObservationIgnored private var wakeAt: Double?
    @ObservationIgnored private var cachedChart: CandleChart?
    @ObservationIgnored private var chartKey: ChartKey?

    private struct ChartKey: Equatable {
        let revision: UInt64
        let timeframe: Timeframe
        let zoom: Zoom
        let maxBars: Int
        let utcOffset: Int
    }

    init(store: SettingsStore, frontmost: Frontmost) throws {
        self.store = store
        core = try RustApp(settingsJSON: store.json, frontmost: frontmost, now: Date().timeIntervalSince1970)
        state = try core.view()
        store.onExternalChange = { [weak self] json in self?.settingsChanged(json) }
    }

    private func settingsChanged(_ json: String) {
        let settings = (try? JSONSerialization.jsonObject(with: Data(json.utf8))) ?? [:]
        drive(try core.send(["type": "settings_changed", "settings": settings]))
    }

    func start() {
        guard heartbeat == nil else { return }
        let timer = Timer(timeInterval: 1, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
        timer.tolerance = 0.1
        RunLoop.main.add(timer, forMode: .common)
        heartbeat = timer
        tick()
    }

    func send(_ event: OverlayEvent) { drive(try core.send(event.input)) }
    func woke() { timedInput("woke") }
    private func tick() { timedInput("tick") }

    private func timedInput(_ kind: String) {
        // The heartbeat carries only a clock; avoid bridging an NSDictionary on every wake.
        drive(try core.send(json: "{\"type\":\"\(kind)\",\"now\":\(Date().timeIntervalSince1970)}"))
    }

    private func drive(_ result: @autoclosure () throws -> CoreReply) {
        do {
            let reply = try result()
            for effect in reply.effects {
                switch effect {
                case .saveSettings(let settings): store.save(settings)
                case .fetch(let fetch): fetchResult(fetch)
                case .render:
                    state = try core.view()
                    onChange()
                }
            }
            arm(reply.nextWake)
        } catch {
            FileHandle.standardError.write(Data("Rust core: \(error)\n".utf8))
        }
    }

    private func arm(_ next: Double?) {
        guard next != wakeAt else { return }
        wakeTimer?.invalidate()
        wakeTimer = nil
        wakeAt = next
        guard let next else { return }
        let timer = Timer(fire: Date(timeIntervalSince1970: next), interval: 0, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.wakeAt = nil
                self?.tick()
            }
        }
        RunLoop.main.add(timer, forMode: .common)
        wakeTimer = timer
    }

    private func fetchResult(_ fetch: CoreEffect.Fetch) {
        Task {
            let request = URLRequest(url: fetch.url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: fetch.timeout)
            let outcome: [String: Any]
            do {
                let (body, response) = try await session.data(for: request)
                outcome = ["type": "answered", "status": (response as? HTTPURLResponse)?.statusCode ?? 0,
                           "body": String(data: body, encoding: .utf8) ?? ""]
            } catch {
                outcome = ["type": "unreachable"]
            }
            var input: [String: Any] = ["type": fetch.timeframe == nil ? "quote_finished" : "candles_finished",
                                       "request": fetch.request, "outcome": outcome, "now": Date().timeIntervalSince1970]
            if let timeframe = fetch.timeframe { input["timeframe"] = timeframe.rawValue }
            drive(try core.send(input))
        }
    }

    /// Quotes and heartbeat renders reuse the decoded bars; only candles, zoom, timeframe, budget or timezone invalidate them.
    func chart(maxBars: Int) -> CandleChart? {
        let offset = TimeZone.current.secondsFromGMT(for: state.chartStart ?? .now)
        let key = ChartKey(revision: state.chartRevision, timeframe: state.settings.timeframe,
                           zoom: state.settings.zoom, maxBars: maxBars, utcOffset: offset)
        guard key != chartKey else { return cachedChart }
        do {
            var chart = try core.view(maxBars: maxBars, utcOffset: offset).chart
            // Merging may omit an oldest partial run across a DST boundary.
            if let start = chart?.timeDomain.lowerBound {
                let exactOffset = TimeZone.current.secondsFromGMT(for: start)
                if exactOffset != offset { chart = try core.view(maxBars: maxBars, utcOffset: exactOffset).chart }
            }
            cachedChart = chart
            chartKey = key
        } catch {
            FileHandle.standardError.write(Data("Rust chart: \(error)\n".utf8))
        }
        return cachedChart
    }
}

/// Persists Settings as a JSON string and applies edits made to the defaults domain from outside the app.
@MainActor
final class SettingsStore {
    static let e2eSuite = "dev.local.btcticker.e2e"
    var onExternalChange: (String) -> Void = { _ in }
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
                self.onExternalChange(self.json)
            }
        }
    }

    var json: String { defaults.btcTickerSettings ?? "{}" }

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

import CTickerCore
import Foundation

public struct CoreReply: Decodable, Sendable {
    public let effects: [CoreEffect]
    public let nextWake: Double?
}

public enum CoreEffect: Decodable, Sendable {
    case saveSettings(Settings)
    case fetch(Fetch)
    case render

    public struct Fetch: Sendable {
        public let request: UInt64
        public let timeframe: Timeframe?
        public let url: URL
        public let timeout: Double
    }

    private enum Keys: String, CodingKey { case type, settings, request, timeframe, url, timeout }
    private enum Kind: String, Decodable { case saveSettings = "save_settings", fetchQuote = "fetch_quote", fetchCandles = "fetch_candles", render }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        switch try c.decode(Kind.self, forKey: .type) {
        case .saveSettings: self = .saveSettings(try c.decode(Settings.self, forKey: .settings))
        case .render: self = .render
        case .fetchQuote, .fetchCandles:
            self = .fetch(Fetch(request: try c.decode(UInt64.self, forKey: .request),
                               timeframe: try c.decodeIfPresent(Timeframe.self, forKey: .timeframe),
                               url: try c.decode(URL.self, forKey: .url), timeout: try c.decode(Double.self, forKey: .timeout)))
        }
    }
}

public struct CoreError: Error, Decodable, CustomStringConvertible {
    public let error: String
    public var description: String { error }
}

/// A single owner of the C handle. Not Sendable: the shell confines it to the main actor.
public final class RustApp {
    private let handle: OpaquePointer
    private let decoder: JSONDecoder

    public init(settingsJSON: String, frontmost: Frontmost, now: Double) throws {
        decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .secondsSince1970
        var error: UnsafeMutablePointer<CChar>?
        let app = settingsJSON.withCString { settings in
            "\"\(frontmost.rawValue)\"".withCString { ticker_create(settings, $0, now, &error) }
        }
        guard let app else {
            throw try Self.readError(error)
        }
        handle = app
    }

    deinit { ticker_destroy(handle) }

    public func send(json: String) throws -> CoreReply {
        try json.withCString { try decode(ticker_handle(handle, $0)) }
    }

    public func send(_ input: [String: Any]) throws -> CoreReply {
        let data = try JSONSerialization.data(withJSONObject: input, options: [.fragmentsAllowed])
        return try send(json: String(decoding: data, as: UTF8.self))
    }

    public func view(maxBars: Int = 0, utcOffset: Int = 0) throws -> CoreView {
        try decode(ticker_view(handle, max(0, maxBars), Int32(clamping: utcOffset)))
    }

    private func decode<T: Decodable>(_ string: UnsafeMutablePointer<CChar>?) throws -> T {
        guard let string else { throw CoreError(error: "Rust returned no response") }
        defer { ticker_string_free(string) }
        let data = Data(bytes: string, count: strlen(string))
        do {
            return try decoder.decode(T.self, from: data)
        } catch {
            if let coreError = try? decoder.decode(CoreError.self, from: data) { throw coreError }
            throw error
        }
    }

    private static func readError(_ string: UnsafeMutablePointer<CChar>?) throws -> CoreError {
        guard let string else { return CoreError(error: "Could not create Rust app") }
        defer { ticker_string_free(string) }
        return try JSONDecoder().decode(CoreError.self, from: Data(bytes: string, count: strlen(string)))
    }
}

extension OverlayEvent {
    public var input: [String: Any] {
        let event: [String: Any]
        switch self {
        case .pressed: event = ["type": "pressed"]
        case .pointerMoved(let distance): event = ["type": "pointer_moved", "distance": distance]
        case .released(let target, let origin):
            let over: String = switch target {
            case .body: "body"
            case .outside: "outside"
            case .timeframe(let timeframe): timeframe.rawValue
            }
            event = ["type": "released", "over": over, "x": origin.x, "y": origin.y]
        case .toggleLock: event = ["type": "toggle_lock"]
        case .zoom(let step): event = ["type": "zoom", "inward": step == .in]
        case .appActivated(let app): event = ["type": "app_activated", "frontmost": app.rawValue]
        case .settingsEdited(let settings): event = ["type": "settings_edited", "settings": settings.object]
        }
        return ["type": "overlay", "event": event]
    }
}

extension Settings {
    public var object: [String: Any] {
        // Settings contains only finite values decoded from core JSON or native controls.
        (try! JSONSerialization.jsonObject(with: Data(json.utf8))) as! [String: Any]
    }
}

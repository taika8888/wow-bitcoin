// End-to-end check against the built app. Usage: swift scripts/e2e.swift "<path>.app" <pidfile>
// Never types, never moves the cursor, and clicks only the overlay's own process; quits everything it launches.
import AppKit
import CoreGraphics
import Foundation
import ImageIO

let arguments = CommandLine.arguments
guard arguments.count == 3 else {
    print("usage: swift scripts/e2e.swift <app bundle> <pid file>")
    exit(2)
}
let appURL = URL(fileURLWithPath: arguments[1])
let pidFile = URL(fileURLWithPath: arguments[2])
let suite = "dev.local.btcticker.e2e"
let settingsKey = "btcTickerSettings"
let wowBundleID = "com.blizzard.worldofwarcraft"
let livePattern = #"^BTC \$\d{1,3}(,\d{3})*  [+-]?\d+\.\d{2}%$"#
let editedPattern = #"^\$\d{1,3}(,\d{3})*\.\d{2}  [+-]?\d+\.\d{2}%$"#
let syntheticClick = Notification.Name("dev.local.btcticker.e2e.click")
let candlesKept = ["1m": 120, "15m": 96, "4h": 90]
let captureDir = URL(fileURLWithPath: FileManager.default.currentDirectoryPath).appendingPathComponent("build")
let workDir = FileManager.default.temporaryDirectory.appendingPathComponent("btcticker-e2e-\(getpid())")
let stateURL = workDir.appendingPathComponent("state.json")

var failures = 0
func report(_ status: String, _ name: String, _ detail: String) {
    print("\(status.padding(toLength: 12, withPad: " ", startingAt: 0)) e2e-\(name)  (\(detail))")
    if status != "PASS" { failures += 1 }
}

@discardableResult
func run(_ tool: String, _ args: [String]) -> String {
    let process = Process()
    let pipe = Pipe()
    process.executableURL = URL(fileURLWithPath: tool)
    process.arguments = args
    process.standardOutput = pipe
    process.standardError = FileHandle.nullDevice
    guard (try? process.run()) != nil else { return "" }
    let output = pipe.fileHandleForReading.readDataToEndOfFile()
    process.waitUntilExit()
    return String(decoding: output, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
}

/// Asks LaunchServices directly; NSWorkspace in a script without an app run loop can report a stale answer.
func frontmostBundleID() -> String {
    let asn = run("/usr/bin/lsappinfo", ["front"])
    let info = run("/usr/bin/lsappinfo", ["info", "-only", "bundleid", asn])
    return info.split(separator: "=").last.map { $0.trimmingCharacters(in: CharacterSet(charactersIn: "\" ")) } ?? "unknown"
}

func waitFor<T>(_ seconds: TimeInterval, _ probe: () -> T?) -> T? {
    let deadline = Date().addingTimeInterval(seconds)
    while Date() < deadline {
        if let value = probe() { return value }
        RunLoop.current.run(until: Date().addingTimeInterval(0.2))
    }
    return probe()
}

func rect(_ json: Any?) -> CGRect? {
    guard let frame = json as? [String: Double] else { return nil }
    return CGRect(x: frame["x"] ?? 0, y: frame["y"] ?? 0, width: frame["width"] ?? 0, height: frame["height"] ?? 0)
}

struct Snapshot {
    let text: String
    let feed: String
    let mode: String
    let locked: Bool
    let visible: Bool
    let windowNumber: Int
    let frame: CGRect
    let timeframe: String?
    let chart: String?
    let candles: Int
    let targets: [String: CGRect]
    let activations: Int
    let opacity: Double
    let fetchedAt: Double?
    let shortcuts: [String]

    static func read() -> Snapshot? {
        guard let data = try? Data(contentsOf: stateURL),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let frame = rect(json["frame"])
        else { return nil }
        return Snapshot(
            text: json["text"] as? String ?? "",
            feed: json["feed"] as? String ?? "",
            mode: json["mode"] as? String ?? "",
            locked: json["locked"] as? Bool ?? false,
            visible: json["visible"] as? Bool ?? false,
            windowNumber: json["windowNumber"] as? Int ?? -1,
            frame: frame,
            timeframe: json["timeframe"] as? String,
            chart: json["chart"] as? String,
            candles: json["candles"] as? Int ?? 0,
            targets: (json["targets"] as? [String: Any] ?? [:]).compactMapValues(rect),
            activations: json["activations"] as? Int ?? -1,
            opacity: json["opacity"] as? Double ?? -1,
            fetchedAt: json["fetchedAt"] as? Double,
            shortcuts: json["shortcuts"] as? [String] ?? []
        )
    }
}

func matches(_ text: String, _ pattern: String) -> Bool {
    text.range(of: pattern, options: .regularExpression) != nil
}

func price(in text: String) -> Double? {
    guard let dollar = text.firstIndex(of: "$") else { return nil }
    let digits = text[text.index(after: dollar)...].prefix { $0.isNumber || $0 == "," || $0 == "." }
    return Double(digits.replacingOccurrences(of: ",", with: ""))
}

func krakenReferencePrice() -> Double? {
    let body = run("/usr/bin/curl", ["-s", "--max-time", "8", "https://api.kraken.com/0/public/Ticker?pair=XBTUSD"])
    guard let json = try? JSONSerialization.jsonObject(with: Data(body.utf8)) as? [String: Any],
          let result = json["result"] as? [String: Any],
          let pair = result.values.first as? [String: Any],
          let last = (pair["c"] as? [String])?.first
    else { return nil }
    return Double(last)
}

struct WindowInfo {
    let index: Int
    let number: Int
    let pid: pid_t
    let layer: Int
    let bounds: CGRect
}

func onScreenWindows() -> [WindowInfo] {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
    return list.enumerated().compactMap { index, window in
        guard let number = window[kCGWindowNumber as String] as? Int,
              let pid = window[kCGWindowOwnerPID as String] as? Int,
              let boundsDict = window[kCGWindowBounds as String] as? NSDictionary,
              let bounds = CGRect(dictionaryRepresentation: boundsDict)
        else { return nil }
        return WindowInfo(index: index, number: number, pid: pid_t(pid), layer: window[kCGWindowLayer as String] as? Int ?? 0, bounds: bounds)
    }
}

func same(_ a: CGRect, _ b: CGRect) -> Bool {
    abs(a.minX - b.minX) < 1 && abs(a.minY - b.minY) < 1 && abs(a.width - b.width) < 1 && abs(a.height - b.height) < 1
}

func describe(_ rect: CGRect) -> String {
    "\(Int(rect.minX)),\(Int(rect.minY)) \(Int(rect.width))x\(Int(rect.height))"
}

func writeSettings(_ json: String) {
    run("/usr/bin/defaults", ["write", suite, settingsKey, "-string", json])
}

try? FileManager.default.createDirectory(at: workDir, withIntermediateDirectories: true)
run("/usr/bin/defaults", ["delete", suite])
let frontBefore = frontmostBundleID()

let app = Process()
app.executableURL = appURL.appendingPathComponent("Contents/MacOS/BTCTicker")
app.environment = ProcessInfo.processInfo.environment.merging([
    "BTCTICKER_STATE_FILE": stateURL.path,
    "BTCTICKER_SETTINGS": #"{"showLabel":true,"showChange":true,"decimals":0,"fontSize":15,"locked":false,"onlyWhenWoWActive":false,"refreshSeconds":300}"#,
]) { $1 }
do {
    try app.run()
} catch {
    report("FAIL", "launch", "could not start \(app.executableURL!.path): \(error)")
    exit(1)
}
try? String(app.processIdentifier).write(to: pidFile, atomically: true, encoding: .utf8)

let live = waitFor(20) { Snapshot.read().flatMap { $0.feed == "live" && matches($0.text, livePattern) ? $0 : nil } }
if let live, let shown = price(in: live.text) {
    let reference = krakenReferencePrice()
    let drift = reference.map { abs(shown - $0) / $0 }
    if !(1_000...10_000_000).contains(shown) {
        report("FAIL", "live-quote", "price \(shown) outside the sane band, text '\(live.text)'")
    } else if let drift, drift > 0.02 {
        report("FAIL", "live-quote", "shows \(shown) but Kraken says \(reference!), \(String(format: "%.2f", drift * 100))% apart")
    } else {
        let check = reference.map { "Kraken now \($0)" } ?? "Kraken cross-check unavailable"
        report("PASS", "live-quote", "'\(live.text)', \(check)")
    }
} else {
    let last = Snapshot.read()
    report("FAIL", "live-quote", "no live snapshot matching the price format within 20s; last: \(last.map { "\($0.feed) '\($0.text)'" } ?? "none")")
}

if let firstFetch = live?.fetchedAt {
    writeSettings(#"{"showLabel":true,"showChange":true,"decimals":0,"fontSize":15,"locked":false,"onlyWhenWoWActive":false,"refreshSeconds":2}"#)
    let edited = Date()
    let refetched = waitFor(4) { Snapshot.read().flatMap { ($0.fetchedAt ?? 0) > firstFetch ? $0 : nil } }
    if let refetched, let at = refetched.fetchedAt {
        report("PASS", "refresh-restarts", "refresh 300s -> 2s: a new quote landed \(String(format: "%.1f", at - edited.timeIntervalSince1970))s after the edit, \(String(format: "%.1f", at - firstFetch))s after the first")
    } else {
        report("FAIL", "refresh-restarts", "refresh 300s -> 2s: no new quote within 4s of the edit; still the one fetched at \(firstFetch)")
    }
} else {
    report("FAIL", "refresh-restarts", "no live quote with a fetch time to start from")
}

let shortcuts = Snapshot.read()?.shortcuts ?? []
let missingShortcuts = ["cmd-w performClose:", "cmd-q terminate:"].filter { !shortcuts.contains($0) }
report(missingShortcuts.isEmpty ? "PASS" : "FAIL", "main-menu (wiring only, no keystroke sent)",
       missingShortcuts.isEmpty ? "main menu dispatches \(shortcuts.joined(separator: ", "))" : "main menu lacks \(missingShortcuts.joined(separator: ", ")); has \(shortcuts)")

let current = Snapshot.read()
let windows = onScreenWindows()
let ours = current.flatMap { snapshot in windows.first { $0.number == snapshot.windowNumber } }
if let current, let ours {
    let ownedByApp = ours.pid == app.processIdentifier
    let boundsAgree = same(ours.bounds, current.frame)
    if ownedByApp, boundsAgree, current.visible, ours.bounds.width > 0 {
        report("PASS", "window-on-screen", "window \(ours.number) at layer \(ours.layer), \(describe(ours.bounds)), matches the snapshot")
    } else {
        report("FAIL", "window-on-screen", "window \(ours.number) pid \(ours.pid) vs app \(app.processIdentifier), CG \(describe(ours.bounds)) vs snapshot \(describe(current.frame)), visible \(current.visible)")
    }
} else {
    report("FAIL", "window-on-screen", "window \(current?.windowNumber ?? -1) from the snapshot is not in the on-screen window list")
}

let wowPIDs = Set(NSRunningApplication.runningApplications(withBundleIdentifier: wowBundleID).map(\.processIdentifier))
if !wowPIDs.isEmpty {
    let gameWindows = windows.filter { wowPIDs.contains($0.pid) && $0.bounds.width > 100 && $0.bounds.height > 100 }
    let covered = ours.map { overlay in gameWindows.filter { $0.bounds.intersects(overlay.bounds) } } ?? []
    if let ours, !covered.isEmpty {
        let detail = "overlay index \(ours.index) layer \(ours.layer) \(describe(ours.bounds)); WoW windows it overlaps: "
            + covered.map { "pid \($0.pid) index \($0.index) layer \($0.layer) \(describe($0.bounds))" }.joined(separator: ", ")
        // A higher layer, not just an earlier index, is what keeps the overlay on top after WoW is clicked and reordered.
        let onTop = covered.allSatisfy { ours.index < $0.index && ours.layer > $0.layer }
        report(onTop ? "PASS" : "FAIL", "above-wow", detail)
    } else if ours == nil {
        report("FAIL", "above-wow", "overlay window not on screen")
    } else {
        report("INCONCLUSIVE", "above-wow", "WoW pids \(wowPIDs.sorted()) have no on-screen window under the overlay (minimized, hidden, windowed elsewhere, or on another Space)")
    }
} else {
    report("INCONCLUSIVE", "above-wow", "World of Warcraft is not running, so the ordering against it was not observed")
}

let frontAfterLaunch = frontmostBundleID()

let before = Snapshot.read()
writeSettings(#"{"showLabel":false,"showChange":true,"decimals":2,"fontSize":30,"locked":true,"onlyWhenWoWActive":false}"#)
let edited = waitFor(5) {
    Snapshot.read().flatMap { $0.locked && matches($0.text, editedPattern) && $0.frame.height > (before?.frame.height ?? .infinity) ? $0 : nil }
}
if let edited, let before {
    report("PASS", "settings-live", "'\(before.text)' \(Int(before.frame.height))pt tall -> '\(edited.text)' \(Int(edited.frame.height))pt tall, locked")
} else {
    let last = Snapshot.read()
    report("FAIL", "settings-live", "snapshot did not reflect the edit within 5s; last: '\(last?.text ?? "none")' locked \(last?.locked ?? false) height \(Int(last?.frame.height ?? 0))")
}

let frontNow = frontmostBundleID()
let shouldShow = frontNow == wowBundleID
writeSettings(#"{"showLabel":false,"showChange":true,"decimals":2,"fontSize":30,"locked":true,"onlyWhenWoWActive":true}"#)
let gated = waitFor(5) { () -> (Snapshot, Bool)? in
    guard let snapshot = Snapshot.read(), snapshot.visible == shouldShow else { return nil }
    let listed = onScreenWindows().contains { $0.number == snapshot.windowNumber }
    return listed == shouldShow ? (snapshot, listed) : nil
}
if let gated {
    report("PASS", "only-when-wow", "frontmost \(frontNow): snapshot visible \(gated.0.visible), on-screen list \(gated.1 ? "has" : "lacks") the overlay")
} else {
    let last = Snapshot.read()
    report("FAIL", "only-when-wow", "frontmost \(frontNow) so visible should be \(shouldShow); snapshot visible \(last?.visible ?? false)")
}

if let area = NSScreen.screens.first?.visibleFrame {
    let primaryHeight = CGDisplayBounds(CGMainDisplayID()).height
    let visible = CGRect(x: area.minX, y: primaryHeight - area.maxY, width: area.width, height: area.height)
    writeSettings(#"{"showLabel":true,"showChange":true,"decimals":0,"fontSize":15,"locked":true,"onlyWhenWoWActive":false,"position":{"x":\#(area.maxX - 20),"y":\#(area.maxY - 5)}}"#)
    let pulledIn = waitFor(5) { Snapshot.read().flatMap { abs($0.frame.maxX - visible.maxX) < 1 && abs($0.frame.minY - visible.minY) < 1 ? $0 : nil } }
    let listed = pulledIn.flatMap { snapshot in onScreenWindows().first { $0.number == snapshot.windowNumber } }
    if let pulledIn, let listed, visible.contains(listed.bounds) {
        report("PASS", "clamped", "a saved spot 20pt from the right and 5pt from the top of the visible area put the window at \(describe(listed.bounds)), flush with the corner of \(describe(visible))")
    } else {
        report("FAIL", "clamped", "saved spot hanging off the top right of \(describe(visible)); window at \(Snapshot.read().map { describe($0.frame) } ?? "unknown"), CG \(listed.map { describe($0.bounds) } ?? "not listed")")
    }
} else {
    report("FAIL", "clamped", "no screen to measure against")
}

let frontAfterEdit = frontmostBundleID()

enum ClickRoute: String {
    case postToPid = "CGEvent postToPid"
    case synthesized = "synthesized NSEvent, not HID"
}
var route: ClickRoute?
var clicks = 0
var fronts: [(before: String, after: String)] = []
var tickerSamples: [CGRect] = []
var cursorOnTarget: [CGPoint] = []
let tickerBundleID = Bundle(url: appURL)?.bundleIdentifier ?? "dev.local.btcticker"

func postToPid(at point: CGPoint) {
    for type in [CGEventType.leftMouseDown, .leftMouseUp] {
        CGEvent(mouseEventSource: CGEventSource(stateID: .privateState), mouseType: type, mouseCursorPosition: point, mouseButton: .left)?
            .postToPid(app.processIdentifier)
    }
}

func synthesize(at point: CGPoint) {
    DistributedNotificationCenter.default().postNotificationName(
        syntheticClick, object: nil, userInfo: ["x": Double(point.x), "y": Double(point.y)], deliverImmediately: true
    )
}

/// Real input first; the synthesized route only once real input is shown not to arrive (no post-event access).
func click(_ target: String, expecting done: @escaping (Snapshot) -> Bool) -> Snapshot? {
    guard let before = Snapshot.read(), let rect = before.targets[target] else { return nil }
    let point = CGPoint(x: rect.midX, y: rect.midY)
    let frontBefore = frontmostBundleID()
    var result: Snapshot?
    if route != .synthesized {
        postToPid(at: point)
        result = waitFor(route == nil ? 2 : 10) { Snapshot.read().flatMap { done($0) ? $0 : nil } }
        if route == nil { route = result == nil ? .synthesized : .postToPid }
    }
    if route == .synthesized {
        synthesize(at: point)
        result = waitFor(10) { Snapshot.read().flatMap { done($0) ? $0 : nil } }
    }
    if result != nil { clicks += 1 }
    fronts.append((frontBefore, frontmostBundleID()))
    // A warped cursor would sit on the target; the user moving the mouse would not.
    if let cursor = CGEvent(source: nil)?.location, hypot(cursor.x - point.x, cursor.y - point.y) < 1 { cursorOnTarget.append(cursor) }
    if let ticker = Snapshot.read()?.targets["ticker"] { tickerSamples.append(ticker) }
    return result
}

let hitGrid = (0...6).flatMap { i in (0...4).map { j in (Double(i) / 6, Double(j) / 4) } }

/// Asks the window server which window takes events at each point; it skips transparent pixels of a click-through window.
/// Without a window-server connection of its own (NSApplication.shared) the query answers 0 everywhere.
func missedHits(_ snapshot: Snapshot) -> [CGPoint] {
    NSApplication.shared.setActivationPolicy(.prohibited)
    let primaryHeight = CGDisplayBounds(CGMainDisplayID()).height
    let area = snapshot.frame.insetBy(dx: 1, dy: 1)
    return hitGrid.map { CGPoint(x: area.minX + area.width * $0.0, y: area.minY + area.height * $0.1) }.filter { point in
        NSWindow.windowNumber(at: NSPoint(x: point.x, y: primaryHeight - point.y), belowWindowWithWindowNumber: 0) != snapshot.windowNumber
    }
}

func charted(_ timeframe: String) -> (Snapshot) -> Bool {
    { $0.mode == "expanded" && $0.timeframe == timeframe && $0.chart == "loaded" && $0.candles == candlesKept[timeframe] }
}

func describeChart(_ snapshot: Snapshot?) -> String {
    guard let snapshot else { return "no snapshot" }
    return "mode \(snapshot.mode) timeframe \(snapshot.timeframe ?? "-") chart \(snapshot.chart ?? "-") candles \(snapshot.candles)"
}

/// Pixels close to the default up (green) and down (red) colors prove candles were drawn, not just modelled.
func inspect(_ url: URL) -> (width: Int, colors: Int, up: Int, down: Int)? {
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil),
          let space = CGColorSpace(name: CGColorSpace.sRGB)
    else { return nil }
    let width = image.width, height = image.height
    var pixels = [UInt8](repeating: 0, count: width * height * 4)
    guard let context = CGContext(data: &pixels, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                                  space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
    else { return nil }
    context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
    var colors = Set<Int>()
    var up = 0, down = 0
    for i in stride(from: 0, to: pixels.count, by: 4) {
        let r = Int(pixels[i]), g = Int(pixels[i + 1]), b = Int(pixels[i + 2])
        colors.insert((r >> 4) << 8 | (g >> 4) << 4 | b >> 4)
        guard pixels[i + 3] > 200 else { continue }
        if g > r + 60, g > b + 40 { up += 1 }
        if r > g + 80, r > b + 80 { down += 1 }
    }
    return (width, colors.count, up, down)
}

func capture(_ snapshot: Snapshot, _ timeframe: String) {
    let url = captureDir.appendingPathComponent("expanded-\(timeframe).png")
    try? FileManager.default.removeItem(at: url)
    RunLoop.current.run(until: Date().addingTimeInterval(0.5))
    run("/usr/sbin/screencapture", ["-x", "-o", "-l", String(snapshot.windowNumber), url.path])
    guard let found = inspect(url) else {
        report("FAIL", "capture-\(timeframe)", "no readable PNG at \(url.path)")
        return
    }
    let detail = "\(url.path): \(found.width)px wide for a \(Int(snapshot.frame.width))pt window, \(found.colors) colors, \(found.up) up-colored and \(found.down) down-colored pixels"
    let ok = found.width >= Int(snapshot.frame.width) && found.colors >= 16 && found.up >= 50 && found.down >= 50
    report(ok ? "PASS" : "FAIL", "capture-\(timeframe)", detail)
}

writeSettings(#"{"showLabel":true,"showChange":true,"decimals":0,"fontSize":15,"locked":true,"onlyWhenWoWActive":false,"timeframe":"h4"}"#)
let collapsed = waitFor(5) {
    Snapshot.read().flatMap { $0.locked && $0.visible && $0.mode == "collapsed" && $0.frame.height < 35 && matches($0.text, livePattern) ? $0 : nil }
}
let tickerHome = collapsed?.targets["ticker"]
if let collapsed, let tickerHome {
    tickerSamples.append(tickerHome)
    let expanded = click("ticker", expecting: charted("4h"))
    let listed = expanded.flatMap { snapshot in onScreenWindows().first { $0.number == snapshot.windowNumber } }
    let screen = CGDisplayBounds(CGMainDisplayID())
    if let expanded, let listed, same(listed.bounds, expanded.frame), screen.contains(expanded.frame) {
        report("PASS", "expand", "locked ticker click opened the last chosen 4h with \(expanded.candles) candles; window \(describe(collapsed.frame)) -> \(describe(listed.bounds)) per CGWindowList, inside the \(describe(screen)) display")
    } else {
        report("FAIL", "expand", "\(describeChart(expanded ?? Snapshot.read())); CG bounds \(listed.map { describe($0.bounds) } ?? "not listed")")
    }
    if let expanded { capture(expanded, "4h") }

    for timeframe in ["15m", "1m"] {
        let shown = click(timeframe, expecting: charted(timeframe))
        if let shown {
            report("PASS", "timeframe-\(timeframe)", "clicked the \(timeframe) control: \(describeChart(shown)), expected \(candlesKept[timeframe]!)")
            capture(shown, timeframe)
        } else {
            report("FAIL", "timeframe-\(timeframe)", "after clicking \(timeframe): \(describeChart(Snapshot.read())), expected \(candlesKept[timeframe]!) loaded candles")
        }
    }

    let folded = click("ticker") { $0.mode == "collapsed" && same($0.frame, collapsed.frame) }
    report(folded != nil ? "PASS" : "FAIL", "collapse",
           "clicked the ticker: \(folded.map { "collapsed back to \(describe($0.frame))" } ?? describeChart(Snapshot.read()))")

    let reopened = click("ticker", expecting: charted("1m"))
    let persisted = run("/usr/bin/defaults", ["read", suite, settingsKey]).contains(#""timeframe":"m1""#)
    if reopened != nil, persisted {
        report("PASS", "reexpand", "reopened on 1m, the last chosen, and the saved settings hold timeframe m1")
    } else {
        report("FAIL", "reexpand", "\(describeChart(Snapshot.read())), saved timeframe m1: \(persisted)")
    }
    _ = click("ticker") { $0.mode == "collapsed" }

    writeSettings(#"{"showLabel":true,"showChange":true,"decimals":0,"fontSize":15,"locked":true,"onlyWhenWoWActive":false,"opacity":0}"#)
    let clear = waitFor(5) { Snapshot.read().flatMap { $0.opacity == 0 && $0.mode == "collapsed" ? $0 : nil } }
    let clearExpanded = clear.flatMap { _ in click("ticker") { $0.mode == "expanded" && $0.opacity == 0 } }
    let probes = [clear, clearExpanded].compactMap { $0.map { ($0.mode, $0.frame, missedHits($0)) } }
    let missed = probes.filter { !$0.2.isEmpty }
    if probes.count == 2, missed.isEmpty {
        report("PASS", "opaque-to-clicks (window-server hit test, not HID)",
               "at background opacity 0, NSWindow.windowNumber(at:) returns the overlay at all \(hitGrid.count) grid points of the collapsed \(describe(probes[0].1)) and expanded \(describe(probes[1].1)) panel")
    } else {
        report("FAIL", "opaque-to-clicks (window-server hit test, not HID)",
               probes.count < 2 ? "opacity 0 edit or expand did not land: \(describeChart(Snapshot.read()))"
                   : missed.map { "\($0.0) \(describe($0.1)): \($0.2.count)/\(hitGrid.count) points fall through, e.g. \($0.2.prefix(3))" }.joined(separator: "; "))
    }
    _ = click("ticker") { $0.mode == "collapsed" }

    let moved = tickerSamples.filter { !same($0, tickerHome) }
    if moved.isEmpty {
        report("PASS", "ticker-anchored", "ticker target stayed at \(describe(tickerHome)) across \(tickerSamples.count) samples in both modes")
    } else {
        report("FAIL", "ticker-anchored", "ticker target \(describe(tickerHome)) moved to \(moved.map(describe).joined(separator: ", "))")
    }

    let label = "click (\((route ?? .synthesized).rawValue))"
    if let route, clicks == fronts.count, cursorOnTarget.isEmpty {
        let fallback = route == .synthesized ? "; postToPid did not reach the app (CGPreflightPostEventAccess \(CGPreflightPostEventAccess()))" : ""
        report("PASS", label, "\(clicks)/\(fronts.count) clicks changed the snapshot as expected; the cursor was never warped onto a target\(fallback)")
    } else {
        report("FAIL", label, "\(clicks)/\(fronts.count) clicks took effect; cursor found on a click target at \(cursorOnTarget)")
    }
} else {
    report("FAIL", "expand", "the overlay never showed a collapsed, locked ticker with a hit target; \(describeChart(Snapshot.read()))")
}

let activations = Snapshot.read()?.activations ?? -1
let samples = [frontBefore, frontAfterLaunch, frontAfterEdit] + fronts.flatMap { [$0.before, $0.after] }
let switches = fronts.filter { $0.before != $0.after }.map { "\($0.before) -> \($0.after)" }
    + (frontBefore == frontAfterLaunch && frontAfterLaunch == frontAfterEdit ? [] : ["\(frontBefore) -> \(frontAfterLaunch) -> \(frontAfterEdit) before the clicks"])
if activations != 0 || samples.contains(tickerBundleID) {
    report("FAIL", "focus-kept", "BTC Ticker became active \(activations) times; frontmost samples: \(samples.joined(separator: ", "))")
} else if switches.isEmpty {
    report("PASS", "focus-kept", "frontmost stayed \(frontBefore) through launch, settings edits and \(fronts.count) clicks; BTC Ticker never became active")
} else {
    report("PASS", "focus-kept", "BTC Ticker never became active and was never frontmost; frontmost changed only between other apps, which a click on the panel cannot cause: \(switches.joined(separator: "; "))")
}

let accepted = NSRunningApplication(processIdentifier: app.processIdentifier)?.terminate() ?? false
let exited = waitFor(10) { app.isRunning ? nil : true } ?? false
if exited {
    report("PASS", "quit", "terminate() \(accepted ? "accepted" : "refused"), process exited with status \(app.terminationStatus)")
} else {
    report("FAIL", "quit", "process \(app.processIdentifier) still running 10s after terminate() (\(accepted ? "accepted" : "refused")), killing it")
    app.terminate()
}

run("/usr/bin/defaults", ["delete", suite])
try? FileManager.default.removeItem(at: workDir)
exit(failures == 0 ? 0 : 1)

// End-to-end check against the built app. Usage: swift scripts/e2e.swift "<path>.app" <pidfile>
// Never clicks, types, or moves the cursor; quits everything it launches.
import AppKit
import CoreGraphics
import Foundation

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

struct Snapshot {
    let text: String
    let feed: String
    let locked: Bool
    let visible: Bool
    let windowNumber: Int
    let frame: CGRect

    static func read() -> Snapshot? {
        guard let data = try? Data(contentsOf: stateURL),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let frame = json["frame"] as? [String: Double]
        else { return nil }
        return Snapshot(
            text: json["text"] as? String ?? "",
            feed: json["feed"] as? String ?? "",
            locked: json["locked"] as? Bool ?? false,
            visible: json["visible"] as? Bool ?? false,
            windowNumber: json["windowNumber"] as? Int ?? -1,
            frame: CGRect(x: frame["x"] ?? 0, y: frame["y"] ?? 0, width: frame["width"] ?? 0, height: frame["height"] ?? 0)
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
    "BTCTICKER_SETTINGS": #"{"showLabel":true,"showChange":true,"decimals":0,"fontSize":15,"locked":false,"onlyWhenWoWActive":false}"#,
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

let current = Snapshot.read()
let windows = onScreenWindows()
let ours = current.flatMap { snapshot in windows.first { $0.number == snapshot.windowNumber } }
if let current, let ours {
    let ownedByApp = ours.pid == app.processIdentifier
    let boundsAgree = abs(ours.bounds.minX - current.frame.minX) < 1 && abs(ours.bounds.minY - current.frame.minY) < 1
        && abs(ours.bounds.width - current.frame.width) < 1 && abs(ours.bounds.height - current.frame.height) < 1
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

let frontAfterEdit = frontmostBundleID()
if frontBefore == frontAfterLaunch, frontBefore == frontAfterEdit {
    report("PASS", "focus-kept", "frontmost stayed \(frontBefore) through launch and a settings edit")
} else {
    report("FAIL", "focus-kept", "frontmost was \(frontBefore), then \(frontAfterLaunch) after launch, \(frontAfterEdit) after the edit")
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

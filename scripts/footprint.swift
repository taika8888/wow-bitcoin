// Memory and CPU of the built app through collapse, every timeframe, the widest zoom and collapse again.
// Usage: swift scripts/footprint.swift "<path>.app" [<path>.app ...] [--runs N]
// Bundles are run in turn, A B A B, so drift and warm caches favor none of them. The overlay sits in the top-right corner.
import AppKit
import Darwin
import Foundation

var bundles: [URL] = []
var runs = 5
var args = CommandLine.arguments.dropFirst()
while let arg = args.popFirst() {
    if arg == "--runs", let n = args.popFirst().flatMap(Int.init) { runs = n } else { bundles.append(URL(fileURLWithPath: arg)) }
}
guard !bundles.isEmpty else {
    print("usage: swift scripts/footprint.swift <app bundle>... [--runs N]")
    exit(2)
}

let suite = "dev.local.btcticker.e2e"
let candlesShown = ["1m": 120, "15m": 96, "4h": 90]
let area = NSScreen.screens.first!.visibleFrame
let seed = #"{"position":{"x":\#(area.maxX - 20),"y":\#(area.maxY - 5)},"locked":true,"refreshSeconds":5,"timeframe":"m15","zoom":2}"#

func sample(_ pid: pid_t) -> (footprint: Double, peak: Double, cpu: Double) {
    var info = rusage_info_v4()
    let ok = withUnsafeMutablePointer(to: &info) {
        $0.withMemoryRebound(to: rusage_info_t?.self, capacity: 1) { proc_pid_rusage(pid, RUSAGE_INFO_V4, $0) }
    }
    guard ok == 0 else { return (.nan, .nan, .nan) }
    var timebase = mach_timebase_info_data_t()
    mach_timebase_info(&timebase)
    let cpu = Double(info.ri_user_time + info.ri_system_time) * Double(timebase.numer) / Double(timebase.denom) / 1e9
    return (Double(info.ri_phys_footprint) / 1_048_576, Double(info.ri_lifetime_max_phys_footprint) / 1_048_576, cpu)
}

func state(_ url: URL) -> [String: Any]? {
    (try? Data(contentsOf: url)).flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
}

func waitFor(_ seconds: TimeInterval, _ probe: () -> Bool) -> Bool {
    let deadline = Date().addingTimeInterval(seconds)
    while Date() < deadline {
        if probe() { return true }
        RunLoop.current.run(until: Date().addingTimeInterval(0.2))
    }
    return probe()
}

func post(_ name: String, _ info: [String: Any]) {
    DistributedNotificationCenter.default().postNotificationName(
        Notification.Name("dev.local.btcticker.e2e.\(name)"), object: nil, userInfo: info, deliverImmediately: true
    )
}

func center(_ url: URL, _ target: String) -> [String: Double]? {
    guard let rect = (state(url)?["targets"] as? [String: [String: Double]])?[target],
          let x = rect["x"], let y = rect["y"], let w = rect["width"], let h = rect["height"]
    else { return nil }
    return ["x": x + w / 2, "y": y + h / 2]
}

func click(_ url: URL, _ target: String, until done: ([String: Any]) -> Bool) -> Bool {
    guard let point = center(url, target) else { return false }
    post("click", point)
    return waitFor(15) { state(url).map(done) ?? false }
}

func charted(_ timeframe: String) -> ([String: Any]) -> Bool {
    { $0["mode"] as? String == "expanded" && $0["timeframe"] as? String == timeframe && $0["chart"] as? String == "loaded"
        && $0["candles"] as? Int == candlesShown[timeframe] }
}

let phases = ["collapsed", "15m", "1m", "4h", "4h-all", "idle-open", "collapsed-again"]

/// One launch: footprint after each phase settles, CPU% over 20s idle with the chart open, and the lifetime peak.
func measure(_ bundle: URL) -> (phases: [String: Double], idleCPU: Double, peak: Double)? {
    let work = FileManager.default.temporaryDirectory.appendingPathComponent("btcticker-footprint-\(getpid())-\(UUID().uuidString)")
    try? FileManager.default.createDirectory(at: work, withIntermediateDirectories: true)
    let stateURL = work.appendingPathComponent("state.json")
    let app = Process()
    app.executableURL = bundle.appendingPathComponent("Contents/MacOS/BTCTicker")
    app.environment = ProcessInfo.processInfo.environment.merging(["BTCTICKER_STATE_FILE": stateURL.path, "BTCTICKER_SETTINGS": seed]) { $1 }
    app.standardError = FileHandle.nullDevice
    guard (try? app.run()) != nil else { return nil }
    defer {
        app.terminate()
        app.waitUntilExit()
        try? FileManager.default.removeItem(at: work)
    }
    let pid = app.processIdentifier
    var out: [String: Double] = [:]
    func settle(_ phase: String) {
        RunLoop.current.run(until: Date().addingTimeInterval(3))
        out[phase] = sample(pid).footprint
    }

    guard waitFor(20, { state(stateURL)?["feed"] as? String == "live" }) else { return fail("no live quote") }
    settle("collapsed")
    guard click(stateURL, "ticker", until: charted("15m")) else { return fail("15m chart never loaded") }
    settle("15m")
    guard click(stateURL, "1m", until: charted("1m")) else { return fail("1m chart never loaded") }
    settle("1m")
    guard click(stateURL, "4h", until: charted("4h")) else { return fail("4h chart never loaded") }
    settle("4h")
    guard let mid = center(stateURL, "chart") else { return fail("no chart target") }
    for _ in 0..<4 {
        post("scroll", ["x": mid["x"]!, "y": mid["y"]!, "delta": -3, "precise": false])
        RunLoop.current.run(until: Date().addingTimeInterval(0.15))
    }
    guard waitFor(10, { state(stateURL)?["zoom"] as? String == "all" && (state(stateURL)?["visibleCandles"] as? Int ?? 0) >= 700 }) else {
        return fail("zoom never reached all")
    }
    settle("4h-all")
    let before = sample(pid).cpu
    RunLoop.current.run(until: Date().addingTimeInterval(20))
    let idleCPU = (sample(pid).cpu - before) / 20 * 100
    out["idle-open"] = sample(pid).footprint
    guard click(stateURL, "ticker", until: { $0["mode"] as? String == "collapsed" }) else { return fail("never collapsed") }
    settle("collapsed-again")
    let peak = sample(pid).peak
    return (out, idleCPU, peak)

    func fail(_ why: String) -> (phases: [String: Double], idleCPU: Double, peak: Double)? {
        print("ERROR \(bundle.lastPathComponent): \(why); last state \(state(stateURL).map { "\($0["mode"] ?? "-") \($0["chart"] ?? "-")" } ?? "none")")
        return nil
    }
}

func row(_ cells: [String]) -> String { cells.map { $0.padding(toLength: 20, withPad: " ", startingAt: 0) }.joined(separator: " ") }

var results: [URL: [(phases: [String: Double], idleCPU: Double, peak: Double)]] = [:]
var errors = 0
for round in 1...runs {
    for bundle in bundles {
        run("/usr/bin/defaults", ["delete", suite])
        guard let result = measure(bundle) else { errors += 1; continue }
        results[bundle, default: []].append(result)
        let cells = phases.map { String(format: "%.1f", result.phases[$0] ?? .nan) }
        print("run \(round) \(bundle.lastPathComponent): " + zip(phases, cells).map { "\($0)=\($1)MB" }.joined(separator: " ")
              + String(format: " idleCPU=%.2f%% peak=%.1fMB", result.idleCPU, result.peak))
    }
}

func run(_ tool: String, _ args: [String]) {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: tool)
    process.arguments = args
    process.standardError = FileHandle.nullDevice
    process.standardOutput = FileHandle.nullDevice
    try? process.run()
    process.waitUntilExit()
}

func summary(_ values: [Double]) -> String {
    let sorted = values.sorted()
    guard !sorted.isEmpty else { return "-" }
    return String(format: "%.1f (%.1f-%.1f)", sorted[sorted.count / 2], sorted.first!, sorted.last!)
}

print("\nmedian (min-max) over runs, footprint in MB")
print(row(["bundle"] + phases + ["idle CPU %", "peak"]))
for bundle in bundles {
    let list = results[bundle] ?? []
    print(row([bundle.lastPathComponent] + phases.map { phase in summary(list.compactMap { $0.phases[phase] }) }
              + [summary(list.map(\.idleCPU)), summary(list.map(\.peak))]) + "  n=\(list.count)")
}
print("errors: \(errors)")
exit(errors == 0 ? 0 : 1)

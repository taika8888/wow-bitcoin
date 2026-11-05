import AppKit
import SwiftUI
import TickerCore

/// Borderless, non-activating, above a maximized game window, and never key, so WoW keeps keyboard focus.
final class OverlayPanel: NSPanel {
    init() {
        super.init(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        level = .floating
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]
        hidesOnDeactivate = false
        becomesKeyOnlyIfNeeded = true
        isOpaque = false
        backgroundColor = .clear
        hasShadow = false
        isMovable = false
        isReleasedWhenClosed = false
        animationBehavior = .none
    }

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

@MainActor
final class OverlayController {
    /// Test-only route for clicks: a screen point, in CGWindowList coordinates, delivered as a synthesized NSEvent.
    static let syntheticClick = Notification.Name("dev.local.btcticker.e2e.click")

    private let model: AppModel
    private let panel = OverlayPanel()
    private let view = OverlayView()
    private let stateFile: URL?
    private var lastSnapshot: Data?

    init(model: AppModel, menu: NSMenu, stateFile: URL?) {
        self.model = model
        self.stateFile = stateFile
        view.menu = menu
        view.onEvent = { [weak model] in model?.send($0) }
        view.isDragging = { [weak model] in model?.overlay.drag == .dragging }
        panel.contentView = view
        if stateFile != nil {
            DistributedNotificationCenter.default().addObserver(forName: Self.syntheticClick, object: nil, queue: .main) { [weak self] note in
                guard let x = note.userInfo?["x"] as? Double, let y = note.userInfo?["y"] as? Double else { return }
                MainActor.assumeIsolated { self?.click(at: CGPoint(x: x, y: y)) }
            }
        }
    }

    func render() {
        let overlay = model.overlay
        let settings = overlay.settings
        let display = TickerDisplay(status: model.status, settings: settings)
        let tickerSize = view.show(display, settings: settings)
        let ticker = NSRect(origin: origin(for: settings.position, size: tickerSize), size: tickerSize)
        var load: CandleLoad?
        var chart: CandleChart?
        var frame = ticker
        if case .expanded(let timeframe) = overlay.mode {
            load = model.charts[timeframe]
            chart = load?.series.flatMap { CandleChart(candles: $0.candles, timeframe: timeframe) }
            let screen = NSScreen.screens.first { $0.frame.intersects(ticker) } ?? NSScreen.screens.first
            let layout = ExpandedLayout(
                tickerFrame: ticker,
                segment: view.segmentSize,
                chart: CGSize(width: settings.chartWidth, height: settings.chartHeight),
                screen: screen?.visibleFrame ?? ticker
            )
            view.expand(layout, timeframe: timeframe, chart: chart, placeholder: Self.placeholder(load, timeframe))
            frame = layout.frame
        } else {
            view.collapse()
        }
        if overlay.drag == .dragging { frame.origin = panel.frame.origin }
        panel.setFrame(frame, display: true)
        if overlay.isVisible {
            panel.orderFrontRegardless()
        } else {
            panel.orderOut(nil)
        }
        writeSnapshot(display: display, overlay: overlay, load: load, candles: chart?.bars.count ?? 0)
    }

    private static func placeholder(_ load: CandleLoad?, _ timeframe: Timeframe) -> String {
        switch load {
        case .failed: "Kraken is not answering, retrying"
        case .loaded: "No \(timeframe.spec.label) candles"
        default: "Loading \(timeframe.spec.label) candles"
        }
    }

    /// A saved origin is used only while some screen still shows the overlay, so unplugging a display cannot strand it.
    private func origin(for saved: ScreenPoint?, size: NSSize) -> NSPoint {
        if let saved {
            let frame = NSRect(x: saved.x, y: saved.y, width: size.width, height: size.height)
            if NSScreen.screens.contains(where: { $0.frame.intersects(frame) }) { return frame.origin }
        }
        guard let area = NSScreen.screens.first?.visibleFrame else { return .zero }
        return NSPoint(x: area.maxX - size.width - 16, y: area.maxY - size.height - 12)
    }

    private func click(at point: CGPoint) {
        let local = NSPoint(x: point.x - panel.frame.minX, y: primaryHeight - point.y - panel.frame.minY)
        for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
            guard let event = NSEvent.mouseEvent(
                with: type, location: local, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: panel.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: type == .leftMouseDown ? 1 : 0
            ) else { continue }
            panel.sendEvent(event)
        }
    }

    private var primaryHeight: CGFloat { NSScreen.screens.first?.frame.height ?? 0 }

    private func globalFrame(_ rect: NSRect) -> Snapshot.Frame {
        Snapshot.Frame(x: rect.minX, y: primaryHeight - rect.maxY, width: rect.width, height: rect.height)
    }

    private func writeSnapshot(display: TickerDisplay, overlay: OverlayState, load: CandleLoad?, candles: Int) {
        guard let stateFile else { return }
        let origin = panel.frame.origin
        let snapshot = Snapshot(
            display: display,
            status: model.status,
            overlay: overlay,
            chart: load,
            candles: candles,
            windowNumber: panel.windowNumber,
            frame: globalFrame(panel.frame),
            targets: view.targets.mapValues { globalFrame($0.offsetBy(dx: origin.x, dy: origin.y)) }
        )
        let data = snapshot.json
        guard data != lastSnapshot else { return }
        lastSnapshot = data
        try? data.write(to: stateFile, options: .atomic)
    }
}

/// The reducer, not this view, decides whether a drag may move the window or what a click means.
final class OverlayView: NSView {
    var onEvent: (OverlayEvent) -> Void = { _ in }
    var isDragging: () -> Bool = { false }
    private var text = NSAttributedString()
    private var settings = TickerCore.Settings()
    private var tickerSize = NSSize.zero
    private var expanded: (layout: ExpandedLayout, timeframe: Timeframe)?
    private var press: (mouse: NSPoint, origin: NSPoint, moved: Bool)?
    private let padding = NSSize(width: 10, height: 4)
    private let chartHost = NSHostingView(rootView: CandleChartView())

    override init(frame: NSRect) {
        super.init(frame: frame)
        chartHost.isHidden = true
        addSubview(chartHost)
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    /// Returns the size of the ticker line, which is the whole overlay while collapsed.
    func show(_ display: TickerDisplay, settings: TickerCore.Settings) -> NSSize {
        let font = NSFont.monospacedDigitSystemFont(ofSize: settings.fontSize, weight: .semibold)
        let fade = display.dimmed ? 0.5 : 1
        let runs = NSMutableAttributedString()
        for segment in display.segments {
            let color = switch segment.tone {
            case .neutral: settings.textColor
            case .up: settings.upColor
            case .down: settings.downColor
            }
            runs.append(NSAttributedString(string: segment.text, attributes: [
                .font: font,
                .foregroundColor: NSColor(color).withAlphaComponent(color.alpha * fade),
            ]))
        }
        text = runs
        self.settings = settings
        let size = text.size()
        tickerSize = NSSize(width: ceil(size.width) + padding.width * 2, height: ceil(size.height) + padding.height * 2)
        needsDisplay = true
        return tickerSize
    }

    var segmentSize: NSSize {
        let widest = Timeframe.allCases.map { label($0, selected: true).size().width }.max() ?? 0
        let height = label(.m1, selected: true).size().height
        return NSSize(width: ceil(widest) + 14, height: min(ceil(height) + 6, tickerSize.height))
    }

    func expand(_ layout: ExpandedLayout, timeframe: Timeframe, chart: CandleChart?, placeholder: String) {
        expanded = (layout, timeframe)
        chartHost.frame = layout.chart
        chartHost.isHidden = false
        let plotWidth = layout.chart.width - 60
        chartHost.rootView = CandleChartView(
            chart: chart,
            placeholder: placeholder,
            settings: settings,
            bodyWidth: max(1, plotWidth / Double(max(chart?.bars.count ?? 1, 1)) * 0.7)
        )
        needsDisplay = true
    }

    func collapse() {
        expanded = nil
        chartHost.isHidden = true
        needsDisplay = true
    }

    /// Hit targets in view coordinates, published in the snapshot so the end-to-end check knows where to click.
    var targets: [String: NSRect] {
        var targets = ["ticker": expanded?.layout.ticker ?? NSRect(origin: .zero, size: tickerSize)]
        for (timeframe, rect) in expanded?.layout.timeframes ?? [:] { targets[timeframe.spec.label] = rect }
        return targets
    }

    private func label(_ timeframe: Timeframe, selected: Bool) -> NSAttributedString {
        NSAttributedString(string: timeframe.spec.label, attributes: [
            .font: NSFont.monospacedDigitSystemFont(ofSize: max(9, settings.fontSize * 0.75), weight: selected ? .bold : .medium),
            .foregroundColor: NSColor(settings.textColor).withAlphaComponent(selected ? 1 : 0.6),
        ])
    }

    override func draw(_ dirtyRect: NSRect) {
        let radius = min(8, tickerSize.height / 2)
        NSColor.black.withAlphaComponent(settings.opacity).setFill()
        NSBezierPath(roundedRect: bounds, xRadius: radius, yRadius: radius).fill()
        let ticker = expanded?.layout.ticker.origin ?? .zero
        text.draw(at: NSPoint(x: ticker.x + padding.width, y: ticker.y + padding.height))
        guard let expanded else { return }
        for timeframe in Timeframe.allCases {
            guard let rect = expanded.layout.timeframes[timeframe] else { continue }
            let selected = timeframe == expanded.timeframe
            NSColor.white.withAlphaComponent(selected ? 0.26 : 0.08).setFill()
            NSBezierPath(roundedRect: rect.insetBy(dx: 1, dy: 0), xRadius: 4, yRadius: 4).fill()
            let title = label(timeframe, selected: selected)
            let size = title.size()
            title.draw(at: NSPoint(x: rect.midX - size.width / 2, y: rect.midY - size.height / 2))
        }
    }

    /// The chart is drawn by a hosted SwiftUI view, but every click and drag belongs to this view.
    override func hitTest(_ point: NSPoint) -> NSView? {
        super.hitTest(point) == nil ? nil : self
    }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func mouseDown(with event: NSEvent) {
        guard let window else { return }
        press = (NSEvent.mouseLocation, window.frame.origin, false)
    }

    override func mouseDragged(with event: NSEvent) {
        guard var current = press, let window else { return }
        let mouse = NSEvent.mouseLocation
        let dx = mouse.x - current.mouse.x
        let dy = mouse.y - current.mouse.y
        if !current.moved, hypot(dx, dy) >= 3 {
            current.moved = true
            press = current
            onEvent(.dragBegan)
        }
        if current.moved, isDragging() {
            window.setFrameOrigin(NSPoint(x: current.origin.x + dx, y: current.origin.y + dy))
        }
    }

    /// A drop stores where the ticker line landed, not the panel, so the saved position means the same thing in both modes.
    override func mouseUp(with event: NSEvent) {
        guard let current = press, let window else { return }
        press = nil
        if current.moved {
            let ticker = expanded?.layout.ticker.origin ?? .zero
            onEvent(.dragEnded(at: ScreenPoint(x: window.frame.minX + ticker.x, y: window.frame.minY + ticker.y)))
        } else if let timeframe = expanded?.layout.timeframe(at: convert(event.locationInWindow, from: nil)) {
            onEvent(.selectTimeframe(timeframe))
        } else {
            onEvent(.click)
        }
    }
}

extension NSColor {
    convenience init(_ rgba: RGBA) {
        self.init(srgbRed: rgba.red, green: rgba.green, blue: rgba.blue, alpha: rgba.alpha)
    }
}

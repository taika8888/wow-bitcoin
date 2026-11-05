import AppKit
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
        // Left at its default, the window server passes clicks on transparent pixels through to WoW; set explicitly, the whole frame takes them.
        ignoresMouseEvents = false
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
    /// Test-only route for scrolls: a screen point, in CGWindowList coordinates, and a wheel delta in lines like a mouse wheel
    /// or in pixels like a trackpad, delivered as an NSEvent made from a CGEvent.
    static let syntheticScroll = Notification.Name("dev.local.btcticker.e2e.scroll")
    /// Test-only: re-posts NSWorkspace's app-activated notification inside the app for a pid, leaving the real frontmost app alone.
    static let syntheticActivation = Notification.Name("dev.local.btcticker.e2e.activated")

    private let model: AppModel
    private let panel = OverlayPanel()
    private let view = OverlayView()
    private let stateFile: URL?
    private var lastSnapshot: Data?
    private var activations = 0
    private var synthesizing = false
    private var foreignPresses = 0

    init(model: AppModel, menu: NSMenu, stateFile: URL?) {
        self.model = model
        self.stateFile = stateFile
        view.menu = menu
        view.onEvent = { [weak self, weak model] event in
            if event == .pressed, self?.synthesizing == false { self?.foreignPresses += 1 }
            model?.send(event)
        }
        view.isDragging = { [weak model] in model?.overlay.drag == .dragging }
        panel.contentView = view
        if stateFile != nil {
            DistributedNotificationCenter.default().addObserver(forName: Self.syntheticClick, object: nil, queue: .main) { [weak self] note in
                guard let x = note.userInfo?["x"] as? Double, let y = note.userInfo?["y"] as? Double else { return }
                MainActor.assumeIsolated { self?.click(at: CGPoint(x: x, y: y)) }
            }
            DistributedNotificationCenter.default().addObserver(forName: Self.syntheticScroll, object: nil, queue: .main) { [weak self] note in
                guard let x = note.userInfo?["x"] as? Double, let y = note.userInfo?["y"] as? Double,
                      let delta = note.userInfo?["delta"] as? Int, let precise = note.userInfo?["precise"] as? Bool
                else { return }
                MainActor.assumeIsolated { self?.scroll(at: CGPoint(x: x, y: y), delta: Int32(delta), precise: precise) }
            }
            DistributedNotificationCenter.default().addObserver(forName: Self.syntheticActivation, object: nil, queue: .main) { note in
                guard let pid = note.userInfo?["pid"] as? Int, let app = NSRunningApplication(processIdentifier: pid_t(pid)) else { return }
                NSWorkspace.shared.notificationCenter.post(
                    name: NSWorkspace.didActivateApplicationNotification, object: NSWorkspace.shared, userInfo: [NSWorkspace.applicationUserInfoKey: app]
                )
            }
            NotificationCenter.default.addObserver(forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated {
                    self?.activations += 1
                    self?.render()
                }
            }
        }
    }

    func render() {
        let overlay = model.overlay
        let settings = overlay.settings
        let display = TickerDisplay(status: model.status, settings: settings)
        let tickerSize = view.show(display, settings: settings)
        let screens = ScreenArea.current
        let ticker = TickerPlacement.frame(saved: settings.position, size: tickerSize, screens: screens)
        var load: CandleLoad?
        var chart: CandleChart?
        var frame = ticker
        if case .expanded(let timeframe) = overlay.mode {
            let screen = TickerPlacement.screen(for: ticker, in: screens) ?? screens.first
            let layout = ExpandedLayout(
                tickerFrame: ticker,
                segment: view.segmentSize,
                chart: CGSize(width: settings.chartWidth, height: settings.chartHeight),
                screen: screen?.visible ?? ticker
            )
            load = model.charts[timeframe]
            chart = load?.series.flatMap { CandleChart(candles: $0.candles, timeframe: timeframe, zoom: settings.zoom, maxBars: layout.maxBars) }
            view.expand(layout, timeframe: timeframe, chart: chart, notice: load?.notice(for: timeframe))
            frame = layout.frame
        } else {
            view.collapse()
        }
        if overlay.drag == .dragging { frame.origin = panel.frame.origin }
        panel.setFrame(frame, display: false)
        if overlay.isVisible {
            panel.orderFrontRegardless()
        } else {
            panel.orderOut(nil)
        }
        writeSnapshot(display: display, overlay: overlay, load: load, chart: chart)
    }

    private func click(at point: CGPoint) {
        let local = NSPoint(x: point.x - panel.frame.minX, y: primaryHeight - point.y - panel.frame.minY)
        for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
            guard let event = NSEvent.mouseEvent(
                with: type, location: local, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: panel.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: type == .leftMouseDown ? 1 : 0
            ) else { continue }
            synthesizing = true
            panel.sendEvent(event)
            synthesizing = false
        }
    }

    /// NSEvent(cgEvent:) leaves a bare scroll CGEvent with no window, so the wheel's fields go onto the CGEvent of a
    /// mouse-moved NSEvent made for the panel, which carries the window and the point in it, as the click route's events do.
    private func scroll(at point: CGPoint, delta: Int32, precise: Bool) {
        let local = NSPoint(x: point.x - panel.frame.minX, y: primaryHeight - point.y - panel.frame.minY)
        guard let wheel = CGEvent(scrollWheelEvent2Source: nil, units: precise ? .pixel : .line, wheelCount: 1, wheel1: delta, wheel2: 0, wheel3: 0),
              let carrier = NSEvent.mouseEvent(
                  with: .mouseMoved, location: local, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                  windowNumber: panel.windowNumber, context: nil, eventNumber: 0, clickCount: 0, pressure: 0
              )?.cgEvent
        else { return }
        carrier.type = .scrollWheel
        for field in [CGEventField.scrollWheelEventDeltaAxis1, .scrollWheelEventPointDeltaAxis1, .scrollWheelEventIsContinuous] {
            carrier.setIntegerValueField(field, value: wheel.getIntegerValueField(field))
        }
        carrier.setDoubleValueField(.scrollWheelEventFixedPtDeltaAxis1, value: wheel.getDoubleValueField(.scrollWheelEventFixedPtDeltaAxis1))
        guard let event = NSEvent(cgEvent: carrier) else { return }
        panel.sendEvent(event)
    }

    private var primaryHeight: CGFloat { NSScreen.screens.first?.frame.height ?? 0 }

    private func globalFrame(_ rect: NSRect) -> Snapshot.Frame {
        Snapshot.Frame(x: rect.minX, y: primaryHeight - rect.maxY, width: rect.width, height: rect.height)
    }

    private static func shortcuts(in menu: NSMenu?) -> [String] {
        (menu?.items ?? []).flatMap { item in
            let own = item.keyEquivalent.isEmpty || item.keyEquivalentModifierMask != .command ? []
                : ["cmd-\(item.keyEquivalent) \(item.action.map(NSStringFromSelector) ?? "none")"]
            return own + shortcuts(in: item.submenu)
        }
    }

    private func writeSnapshot(display: TickerDisplay, overlay: OverlayState, load: CandleLoad?, chart: CandleChart?) {
        guard let stateFile else { return }
        let origin = panel.frame.origin
        var snapshot = Snapshot(
            display: display,
            status: model.status,
            overlay: overlay,
            load: load,
            chart: chart,
            windowNumber: panel.windowNumber,
            frame: globalFrame(panel.frame),
            targets: view.targets.mapValues { globalFrame($0.offsetBy(dx: origin.x, dy: origin.y)) },
            activations: activations
        )
        snapshot.shortcuts = Self.shortcuts(in: NSApp.mainMenu)
        snapshot.foreignPresses = foreignPresses
        let data = snapshot.json
        guard data != lastSnapshot else { return }
        lastSnapshot = data
        try? data.write(to: stateFile, options: .atomic)
    }
}

/// The reducer, not this view, decides whether a press is a drag or a click, what a click means, and what a zoom step changes.
final class OverlayView: PaintedView {
    var onEvent: (OverlayEvent) -> Void = { _ in }
    var isDragging: () -> Bool = { false }
    private var text = NSAttributedString()
    private var settings = TickerCore.Settings()
    private var tickerSize = NSSize.zero
    private var expanded: (layout: ExpandedLayout, timeframe: Timeframe)?
    private var press: (mouse: NSPoint, origin: NSPoint)?
    /// Trackpad scroll or pinch gathered toward the next zoom step, in steps.
    private var zoomTravel = 0.0
    private let padding = NSSize(width: 10, height: 4)
    private var chartView: CandleChartView?

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
        if runs != text || settings != self.settings { needsDisplay = true }
        text = runs
        self.settings = settings
        let size = text.size()
        tickerSize = NSSize(width: ceil(size.width) + padding.width * 2, height: ceil(size.height) + padding.height * 2)
        return tickerSize
    }

    var segmentSize: NSSize {
        let widest = Timeframe.allCases.map { label($0, selected: true).size().width }.max() ?? 0
        let height = label(.m1, selected: true).size().height
        return NSSize(width: ceil(widest) + 14, height: min(ceil(height) + 6, tickerSize.height))
    }

    func expand(_ layout: ExpandedLayout, timeframe: Timeframe, chart: CandleChart?, notice: ChartNotice?) {
        if expanded?.layout != layout || expanded?.timeframe != timeframe { needsDisplay = true }
        expanded = (layout, timeframe)
        let chartView = self.chartView ?? CandleChartView()
        if self.chartView == nil {
            addSubview(chartView)
            self.chartView = chartView
        }
        chartView.frame = layout.chart
        chartView.chart = chart
        chartView.notice = notice
        chartView.settings = settings
        chartView.bodyWidth = max(1, layout.plotWidth / Double(max(chart?.bars.count ?? 1, 1)) * 0.7)
    }

    func collapse() {
        guard expanded != nil else { return }
        expanded = nil
        chartView?.removeFromSuperview()
        chartView = nil
        needsDisplay = true
    }

    /// Hit targets in view coordinates, published in the snapshot so the end-to-end check knows where to click.
    var targets: [String: NSRect] {
        var targets = ["ticker": expanded?.layout.ticker ?? NSRect(origin: .zero, size: tickerSize)]
        for (timeframe, rect) in expanded?.layout.timeframes ?? [:] { targets[timeframe.spec.label] = rect }
        if let chart = expanded?.layout.chart { targets["chart"] = chart }
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

    /// The chart is drawn by a subview, but every click, drag and scroll belongs to this view.
    override func hitTest(_ point: NSPoint) -> NSView? {
        super.hitTest(point) == nil ? nil : self
    }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func mouseDown(with event: NSEvent) {
        guard let window else { return }
        press = (NSEvent.mouseLocation, window.frame.origin)
        onEvent(.pressed)
    }

    override func mouseDragged(with event: NSEvent) {
        guard let press, let window else { return }
        let mouse = NSEvent.mouseLocation
        let dx = mouse.x - press.mouse.x
        let dy = mouse.y - press.mouse.y
        onEvent(.pointerMoved(distance: hypot(dx, dy)))
        if isDragging() {
            window.setFrameOrigin(NSPoint(x: press.origin.x + dx, y: press.origin.y + dy))
        }
    }

    /// Over the chart, scrolling up zooms in. A mouse wheel steps once per notch. A trackpad steps once per `scrollPerStep`
    /// points of travel, and its momentum after the fingers lift is ignored so a flick cannot run the whole ladder.
    override func scrollWheel(with event: NSEvent) {
        guard overChart(event) else { return super.scrollWheel(with: event) }
        if !event.hasPreciseScrollingDeltas {
            if event.scrollingDeltaY != 0 { onEvent(.zoom(event.scrollingDeltaY > 0 ? .in : .out)) }
        } else if event.momentumPhase.isEmpty {
            zoom(by: event.scrollingDeltaY / Self.scrollPerStep, restart: event.phase == .began)
        }
    }

    /// Pinching out zooms in, one step per `pinchPerStep` of magnification.
    override func magnify(with event: NSEvent) {
        guard overChart(event) else { return super.magnify(with: event) }
        zoom(by: event.magnification / Self.pinchPerStep, restart: event.phase == .began)
    }

    private static let scrollPerStep = 50.0
    private static let pinchPerStep = 0.25

    private func overChart(_ event: NSEvent) -> Bool {
        expanded?.layout.chart.contains(convert(event.locationInWindow, from: nil)) == true
    }

    private func zoom(by steps: Double, restart: Bool) {
        if restart { zoomTravel = 0 }
        zoomTravel += steps
        while abs(zoomTravel) >= 1 {
            let step: Zoom.Step = zoomTravel > 0 ? .in : .out
            zoomTravel -= step == .in ? 1 : -1
            onEvent(.zoom(step))
        }
    }

    /// A drop stores where the ticker line landed, not the panel, so the saved position means the same thing in both modes.
    override func mouseUp(with event: NSEvent) {
        guard press != nil, let window else { return }
        press = nil
        let point = convert(event.locationInWindow, from: nil)
        let target: PressTarget = if !bounds.contains(point) {
            .outside
        } else if let timeframe = expanded?.layout.timeframe(at: point) {
            .timeframe(timeframe)
        } else {
            .body
        }
        let ticker = (expanded?.layout.ticker ?? NSRect(origin: .zero, size: tickerSize)).offsetBy(dx: window.frame.minX, dy: window.frame.minY)
        let screens = ScreenArea.current
        let screen = TickerPlacement.screen(for: ticker, in: screens) ?? TickerPlacement.screen(for: window.frame, in: screens)
        let landed = screen.map { TickerPlacement.clamp(ticker, into: $0.visible) } ?? ticker
        onEvent(.released(over: target, tickerOrigin: ScreenPoint(x: landed.minX, y: landed.minY)))
    }
}

extension ScreenArea {
    static var current: [ScreenArea] {
        NSScreen.screens.map { ScreenArea(frame: $0.frame, visible: $0.visibleFrame) }
    }
}

extension NSColor {
    convenience init(_ rgba: RGBA) {
        self.init(srgbRed: rgba.red, green: rgba.green, blue: rgba.blue, alpha: rgba.alpha)
    }
}

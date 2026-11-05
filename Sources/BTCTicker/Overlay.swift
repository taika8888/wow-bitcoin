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
        isMovable = false
        isReleasedWhenClosed = false
        animationBehavior = .none
    }

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

@MainActor
final class OverlayController {
    private let model: AppModel
    private let panel = OverlayPanel()
    private let view = TickerView()
    private let stateFile: URL?
    private var lastSnapshot: Data?

    init(model: AppModel, menu: NSMenu, stateFile: URL?) {
        self.model = model
        self.stateFile = stateFile
        view.menu = menu
        view.onEvent = { [weak model] in model?.send($0) }
        view.isDragging = { [weak model] in model?.overlay.drag == .dragging }
        panel.contentView = view
    }

    func render() {
        let overlay = model.overlay
        let display = TickerDisplay(status: model.status, settings: overlay.settings)
        view.show(display, settings: overlay.settings)
        let size = view.fittingSize
        let origin = overlay.drag == .dragging ? panel.frame.origin : origin(for: overlay.settings.position, size: size)
        panel.setFrame(NSRect(origin: origin, size: size), display: true)
        if overlay.isVisible {
            panel.orderFrontRegardless()
        } else {
            panel.orderOut(nil)
        }
        writeSnapshot(display: display, overlay: overlay)
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

    private func writeSnapshot(display: TickerDisplay, overlay: OverlayState) {
        guard let stateFile else { return }
        let frame = panel.frame
        let primaryHeight = NSScreen.screens.first?.frame.height ?? 0
        let snapshot = Snapshot(
            display: display,
            status: model.status,
            overlay: overlay,
            windowNumber: panel.windowNumber,
            frame: Snapshot.Frame(x: frame.minX, y: primaryHeight - frame.maxY, width: frame.width, height: frame.height)
        )
        let data = snapshot.json
        guard data != lastSnapshot else { return }
        lastSnapshot = data
        try? data.write(to: stateFile, options: .atomic)
    }
}

/// Draws the ticker and turns mouse input into overlay events. The reducer decides whether a drag may move the window.
final class TickerView: NSView {
    var onEvent: (OverlayEvent) -> Void = { _ in }
    var isDragging: () -> Bool = { false }
    private var text = NSAttributedString()
    private var background = NSColor.clear
    private var press: (mouse: NSPoint, origin: NSPoint, moved: Bool)?
    private let padding = NSSize(width: 10, height: 4)

    func show(_ display: TickerDisplay, settings: Settings) {
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
        background = NSColor.black.withAlphaComponent(settings.opacity)
        needsDisplay = true
    }

    override var fittingSize: NSSize {
        let size = text.size()
        return NSSize(width: ceil(size.width) + padding.width * 2, height: ceil(size.height) + padding.height * 2)
    }

    override func draw(_ dirtyRect: NSRect) {
        let radius = min(8, bounds.height / 2)
        background.setFill()
        NSBezierPath(roundedRect: bounds, xRadius: radius, yRadius: radius).fill()
        text.draw(at: NSPoint(x: padding.width, y: padding.height))
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

    override func mouseUp(with event: NSEvent) {
        guard let current = press, let window else { return }
        press = nil
        onEvent(current.moved ? .dragEnded(at: ScreenPoint(x: window.frame.minX, y: window.frame.minY)) : .click)
    }
}

extension NSColor {
    convenience init(_ rgba: RGBA) {
        self.init(srgbRed: rgba.red, green: rgba.green, blue: rgba.blue, alpha: rgba.alpha)
    }
}

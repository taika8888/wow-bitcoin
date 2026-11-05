import AppKit
import TickerCore

final class CandleChartView: PaintedView {
    var chart: CandleChart? { didSet { if chart != oldValue { needsDisplay = true } } }
    var notice: ChartNotice? { didSet { if notice != oldValue { needsDisplay = true } } }
    var settings = TickerCore.Settings() { didSet { if settings != oldValue { needsDisplay = true } } }
    var bodyWidth = 2.0 { didSet { if bodyWidth != oldValue { needsDisplay = true } } }

    private static let labelFont = NSFont.systemFont(ofSize: 10)
    private static let staleFont = NSFont.systemFont(ofSize: 10, weight: .semibold)
    private static let timeLabel = DateFormatter()

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        let text = NSColor(settings.textColor)
        guard let chart else {
            guard case .placeholder(let message) = notice else { return }
            let label = NSAttributedString(string: message, attributes: [.font: Self.labelFont, .foregroundColor: text.withAlphaComponent(text.alphaComponent * 0.6)])
            let size = label.size()
            label.draw(at: NSPoint(x: bounds.midX - size.width / 2, y: bounds.midY - size.height / 2))
            return
        }
        let labelAttributes: [NSAttributedString.Key: Any] = [.font: Self.labelFont, .foregroundColor: text.withAlphaComponent(text.alphaComponent * 0.7)]
        let labelHeight = ceil(NSAttributedString(string: "0", attributes: labelAttributes).size().height)
        let plot = NSRect(x: 0, y: 0, width: bounds.width - ExpandedLayout.priceAxisWidth, height: bounds.height - labelHeight - 4)
        guard plot.width > 0, plot.height > 0 else { return }
        let times = chart.timeDomain.lowerBound.timeIntervalSince1970...chart.timeDomain.upperBound.timeIntervalSince1970
        func x(_ date: Date) -> Double {
            plot.minX + (date.timeIntervalSince1970 - times.lowerBound) / (times.upperBound - times.lowerBound) * plot.width
        }
        func y(_ price: Double) -> Double {
            plot.minY + (chart.priceDomain.upperBound - price) / (chart.priceDomain.upperBound - chart.priceDomain.lowerBound) * plot.height
        }

        let grid = NSBezierPath()
        grid.lineWidth = 1
        let utcOffset = TimeZone.current.secondsFromGMT(for: chart.timeDomain.lowerBound)
        let timeTicks = chart.timeTicks(utcOffset: utcOffset)
        if timeTicks.count > 1 {
            Self.timeLabel.setLocalizedDateFormatFromTemplate(timeTicks[1].timeIntervalSince(timeTicks[0]) < 86400 ? "jmm" : "MMMd")
        }
        var labelEnd = -Double.infinity
        for tick in timeTicks {
            let tickX = x(tick).rounded() + 0.5
            grid.move(to: NSPoint(x: tickX, y: plot.minY))
            grid.line(to: NSPoint(x: tickX, y: plot.maxY))
            let label = NSAttributedString(string: Self.timeLabel.string(from: tick), attributes: labelAttributes)
            let labelX = min(tickX + 2, plot.maxX - label.size().width)
            guard labelX >= labelEnd + 6 else { continue }
            label.draw(at: NSPoint(x: labelX, y: plot.maxY + 2))
            labelEnd = labelX + label.size().width
        }
        for tick in chart.priceTicks {
            let tickY = y(tick).rounded() + 0.5
            grid.move(to: NSPoint(x: plot.minX, y: tickY))
            grid.line(to: NSPoint(x: plot.maxX, y: tickY))
            let label = NSAttributedString(string: TickerDisplay.price(tick, decimals: 0), attributes: labelAttributes)
            label.draw(at: NSPoint(x: plot.maxX + 4, y: min(max(tickY - labelHeight / 2, 0), plot.maxY - labelHeight)))
        }
        text.withAlphaComponent(text.alphaComponent * 0.12).setStroke()
        grid.stroke()

        for bar in chart.bars {
            color(bar.tone).set()
            let barX = x(bar.time)
            NSRect(x: barX - 0.5, y: y(bar.high), width: 1, height: y(bar.low) - y(bar.high)).fill()
            NSRect(x: barX - bodyWidth / 2, y: y(bar.bodyHigh), width: bodyWidth, height: y(bar.bodyLow) - y(bar.bodyHigh)).fill()
        }

        let last = NSBezierPath()
        last.lineWidth = 1
        last.setLineDash([4, 3], count: 2, phase: 0)
        let lastY = y(chart.lastPrice).rounded() + 0.5
        last.move(to: NSPoint(x: plot.minX, y: lastY))
        last.line(to: NSPoint(x: plot.maxX, y: lastY))
        text.withAlphaComponent(text.alphaComponent * 0.75).setStroke()
        last.stroke()

        if case .stale(let marker) = notice {
            let label = NSAttributedString(string: marker, attributes: [.font: Self.staleFont, .foregroundColor: NSColor(settings.downColor)])
            let size = label.size()
            let capsule = NSRect(x: 0, y: 0, width: ceil(size.width) + 10, height: ceil(size.height) + 2)
            NSColor.black.withAlphaComponent(0.6).setFill()
            NSBezierPath(roundedRect: capsule, xRadius: capsule.height / 2, yRadius: capsule.height / 2).fill()
            label.draw(at: NSPoint(x: 5, y: 1))
        }
    }

    private func color(_ tone: Tone) -> NSColor {
        let rgba = switch tone {
        case .up: settings.upColor
        case .down: settings.downColor
        case .neutral: settings.textColor
        }
        return NSColor(rgba)
    }
}

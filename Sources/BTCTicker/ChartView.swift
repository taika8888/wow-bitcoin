import Charts
import SwiftUI
import TickerCore

struct CandleChartView: View {
    var chart: CandleChart?
    var notice: ChartNotice?
    var settings = TickerCore.Settings()
    var bodyWidth = 2.0

    var body: some View {
        let text = Color(nsColor: NSColor(settings.textColor))
        if let chart {
            Chart {
                ForEach(chart.bars, id: \.time) { bar in
                    RuleMark(x: .value("Time", bar.time), yStart: .value("Low", bar.low), yEnd: .value("High", bar.high))
                        .lineStyle(StrokeStyle(lineWidth: 1))
                        .foregroundStyle(color(bar.tone))
                    RectangleMark(
                        x: .value("Time", bar.time),
                        yStart: .value("Body low", bar.bodyLow),
                        yEnd: .value("Body high", bar.bodyHigh),
                        width: .fixed(bodyWidth)
                    )
                    .foregroundStyle(color(bar.tone))
                }
                RuleMark(y: .value("Last", chart.lastPrice))
                    .lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 3]))
                    .foregroundStyle(text.opacity(0.75))
            }
            .chartXScale(domain: chart.timeDomain)
            .chartYScale(domain: chart.priceDomain)
            .chartXAxis {
                AxisMarks(values: .automatic(desiredCount: 4)) { _ in
                    AxisGridLine().foregroundStyle(text.opacity(0.12))
                    AxisValueLabel().foregroundStyle(text.opacity(0.7))
                }
            }
            .chartYAxis {
                AxisMarks(position: .trailing, values: .automatic(desiredCount: 4)) { value in
                    AxisGridLine().foregroundStyle(text.opacity(0.12))
                    AxisValueLabel {
                        if let price = value.as(Double.self) {
                            Text(TickerDisplay.price(price, decimals: 0)).foregroundStyle(text.opacity(0.7))
                        }
                    }
                }
            }
            .overlay(alignment: .topLeading) {
                if case .stale(let marker) = notice {
                    Text(marker)
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(Color(nsColor: NSColor(settings.downColor)))
                        .padding(.horizontal, 5)
                        .padding(.vertical, 1)
                        .background(.black.opacity(0.6), in: Capsule())
                }
            }
        } else {
            Text(placeholderText)
                .font(.caption)
                .foregroundStyle(text.opacity(0.6))
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private var placeholderText: String {
        if case .placeholder(let text) = notice { text } else { "" }
    }

    private func color(_ tone: Tone) -> Color {
        let rgba = switch tone {
        case .up: settings.upColor
        case .down: settings.downColor
        case .neutral: settings.textColor
        }
        return Color(nsColor: NSColor(rgba))
    }
}

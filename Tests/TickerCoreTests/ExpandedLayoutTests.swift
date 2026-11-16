import CoreGraphics
import Foundation
import Testing
import TickerCore

@Suite struct ExpandedLayoutTests {
    private let screen = CGRect(x: 0, y: 0, width: 1728, height: 1085)
    private let segment = CGSize(width: 34, height: 19)
    private let chart = CGSize(width: 360, height: 180)

    private func layout(tickerAt origin: CGPoint) -> (ExpandedLayout, CGRect) {
        let ticker = CGRect(origin: origin, size: CGSize(width: 182, height: 27))
        return (ExpandedLayout(tickerFrame: ticker, segment: segment, chart: chart, screen: screen), ticker)
    }

    private static let corners = [CGPoint(x: 20, y: 20), CGPoint(x: 1500, y: 20), CGPoint(x: 20, y: 1040), CGPoint(x: 1530, y: 1046)]

    @Test(arguments: corners)
    func theTickerStaysPutAndTheChartOpensTowardTheMiddle(origin: CGPoint) {
        let (layout, ticker) = layout(tickerAt: origin)
        let tickerOnScreen = layout.ticker.offsetBy(dx: layout.frame.minX, dy: layout.frame.minY)
        let chartOnScreen = layout.chart.offsetBy(dx: layout.frame.minX, dy: layout.frame.minY)
        #expect(tickerOnScreen == ticker)
        #expect(screen.contains(layout.frame))
        if ticker.midY > screen.midY {
            #expect(chartOnScreen.maxY <= ticker.minY, "a ticker in the top half opens downward")
        } else {
            #expect(chartOnScreen.minY >= ticker.maxY, "a ticker in the bottom half opens upward")
        }
        if ticker.midX < screen.midX {
            #expect(layout.frame.minX == ticker.minX && layout.frame.maxX > ticker.maxX, "a ticker on the left opens rightward")
        } else {
            #expect(layout.frame.maxX == ticker.maxX && layout.frame.minX < ticker.minX, "a ticker on the right opens leftward")
        }
        #expect(layout.chart.height == chart.height && layout.chart.width >= chart.width)
        #expect(layout.maxBars == 150, "the default 360pt chart fits 150 bars at 2pt each past the price axis")
    }

    @Test(arguments: corners)
    func timeframeTargetsAreDistinctAndHitTestable(origin: CGPoint) {
        let (layout, _) = layout(tickerAt: origin)
        let panel = CGRect(origin: .zero, size: layout.frame.size)
        let rects = Timeframe.allCases.map { layout.timeframes[$0]! }
        #expect(zip(rects, rects.dropFirst()).allSatisfy { $0.maxX <= $1.minX }, "1m, 15m, 4h read left to right")
        for (timeframe, rect) in zip(Timeframe.allCases, rects) {
            #expect(panel.contains(rect))
            #expect(!rect.intersects(layout.ticker) && !rect.intersects(layout.chart))
            #expect(layout.timeframe(at: CGPoint(x: rect.midX, y: rect.midY)) == timeframe)
        }
        #expect(layout.timeframe(at: CGPoint(x: layout.ticker.midX, y: layout.ticker.midY)) == nil)
        #expect(!layout.chart.intersects(layout.ticker))
    }

    @Test func aScreenTooSmallForTheChartKeepsThePanelOnScreen() {
        let small = CGRect(x: 0, y: 0, width: 400, height: 220)
        let ticker = CGRect(x: 210, y: 190, width: 182, height: 27)
        let layout = ExpandedLayout(tickerFrame: ticker, segment: segment, chart: chart, screen: small)
        #expect(layout.frame.minX >= small.minX && layout.frame.minY >= small.minY)
    }
}

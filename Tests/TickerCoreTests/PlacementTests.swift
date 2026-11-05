import CoreGraphics
import Testing
import TickerCore

/// A laptop panel with a 33pt menu bar and a 70pt Dock, and an external display to its right, taller and with no Dock.
private let laptop = ScreenArea(frame: CGRect(x: 0, y: 0, width: 1728, height: 1117), visible: CGRect(x: 0, y: 70, width: 1728, height: 1014))
private let external = ScreenArea(frame: CGRect(x: 1728, y: -200, width: 2560, height: 1440), visible: CGRect(x: 1728, y: -200, width: 2560, height: 1415))
private let screens = [laptop, external]
private let size = CGSize(width: 180, height: 27)

private func placed(_ x: Double, _ y: Double, size: CGSize = size) -> CGRect {
    TickerPlacement.frame(saved: ScreenPoint(x: x, y: y), size: size, screens: screens)
}

@Suite struct TickerPlacementTests {
    @Test func nothingSavedSitsTopCenterOfTheFirstScreenClearOfTheMinimap() {
        let frame = TickerPlacement.frame(saved: nil, size: size, screens: screens)
        #expect(frame == CGRect(x: 864 - 90, y: 1084 - 12 - 27, width: 180, height: 27))
    }

    @Test func aSpotWellInsideIsKeptExactly() {
        #expect(placed(400, 500) == CGRect(x: 400, y: 500, width: 180, height: 27))
        #expect(placed(3000, 0) == CGRect(x: 3000, y: 0, width: 180, height: 27))
    }

    @Test func aTickerFlushRightStaysWholeWhenItsTextWidens() {
        let flush = placed(1728 - 180, 600)
        let wider = placed(1728 - 180, 600, size: CGSize(width: 260, height: 43))
        #expect(flush.maxX == 1728)
        #expect(wider.maxX == 1728 && wider.width == 260, "grows leftward instead of past the edge")
    }

    @Test(arguments: [(-170.0, 600.0), (900, 1100), (900, 20), (1600, 1100)])
    func aSpotHangingOffTheVisibleAreaIsPulledIn(x: Double, y: Double) {
        let frame = placed(x, y)
        #expect(laptop.visible.contains(frame), "\(frame) left the visible area \(laptop.visible)")
        #expect(frame.size == size)
    }

    @Test func aTickerStraddlingTwoScreensMovesOntoTheOneHoldingMostOfIt() {
        #expect(external.visible.contains(placed(1728 - 40, 300)))
        #expect(laptop.visible.contains(placed(1728 - 140, 300)))
    }

    @Test func aSpotOnADisplayThatIsGoneFallsBackToTheDefault() {
        #expect(placed(9000, 9000) == TickerPlacement.frame(saved: nil, size: size, screens: screens))
    }

    @Test func aTickerWiderThanTheScreenKeepsItsLeftAndTopEdgesVisible() {
        let small = ScreenArea(frame: CGRect(x: 0, y: 0, width: 300, height: 200), visible: CGRect(x: 0, y: 0, width: 300, height: 180))
        let frame = TickerPlacement.frame(saved: ScreenPoint(x: 100, y: 100), size: CGSize(width: 400, height: 300), screens: [small])
        #expect(frame.minX == 0 && frame.maxY == 180)
    }
}

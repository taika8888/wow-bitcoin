import Testing
import TickerCore

@Suite struct ShellClockTests {
    @Test func aWallClockSetBackDoesNotStopTime() {
        var wall = 1000.0
        var nanos: UInt64 = 0
        var clock = ShellClock(wall: { wall }, monotonic: { nanos })
        #expect(clock.now() == 1000)
        wall = 900
        nanos = 2_000_000_000
        #expect(clock.now() == 1002)
        wall = 901
        nanos = 3_000_000_000
        #expect(clock.now() == 1003)
    }

    @Test func aWallClockThatJumpsForwardIsFollowed() {
        var wall = 1000.0
        var nanos: UInt64 = 0
        var clock = ShellClock(wall: { wall }, monotonic: { nanos })
        _ = clock.now()
        wall = 1600
        nanos = 1_000_000_000
        #expect(clock.now() == 1600)
        wall = 1601
        nanos = 2_000_000_000
        #expect(clock.now() == 1601)
    }

    @Test func theRealClockNeverGoesBack() {
        var clock = ShellClock()
        var previous = clock.now()
        for _ in 0..<1000 {
            let next = clock.now()
            #expect(next >= previous)
            previous = next
        }
    }
}

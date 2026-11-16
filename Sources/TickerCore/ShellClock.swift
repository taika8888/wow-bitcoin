import Darwin
import Foundation

/// The time the shell gives the core, in seconds since 1970. The core keeps the largest time it has seen, so a wall
/// clock set back by D seconds would freeze fetching and the stale check for D seconds while the price still read as
/// live. This clock counts on from its last reading with a clock that keeps going through sleep, and follows the wall
/// clock only forward, so quote times stay comparable with Kraken's candle times.
public struct ShellClock {
    private var last: (seconds: Double, nanos: UInt64)?
    private let wall: () -> Double
    private let monotonic: () -> UInt64

    /// `monotonic` is in nanoseconds. On macOS `CLOCK_MONOTONIC` keeps counting while the system sleeps, unlike
    /// `CLOCK_UPTIME_RAW`.
    public init(
        wall: @escaping () -> Double = { Date().timeIntervalSince1970 },
        monotonic: @escaping () -> UInt64 = { clock_gettime_nsec_np(CLOCK_MONOTONIC) }
    ) {
        self.wall = wall
        self.monotonic = monotonic
    }

    public mutating func now() -> Double {
        let nanos = monotonic()
        let wall = wall()
        let seconds = last.map { max(wall, $0.seconds + Double(nanos &- $0.nanos) / 1e9) } ?? wall
        last = (seconds, nanos)
        return seconds
    }
}

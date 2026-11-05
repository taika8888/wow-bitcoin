import Testing
import Foundation
@testable import TickerCore
@Test func probeWorks() throws {
    #expect(probe == 1)
    #expect(Bundle.module.url(forResource: "ticker", withExtension: "json", subdirectory: "Fixtures/kraken") != nil)
}

// A stand-in for World of Warcraft, for scripts/e2e.swift when the game is not running. e2e.swift builds it into an
// app bundle with the game's bundle identifier, so the ticker takes it for the game, as the Windows check's copy named
// Wow.exe does. It shows one plain dark window at normal level in the top-right corner, under the overlay. It is
// background-only (LSBackgroundOnly), because an app started straight from its bundle's executable otherwise becomes
// the frontmost app, even as an accessory. Usage: <executable> <width> <height>
import AppKit

let arguments = CommandLine.arguments
let size = NSSize(width: Double(arguments.dropFirst().first ?? "") ?? 480, height: Double(arguments.dropFirst(2).first ?? "") ?? 300)
let parent = getppid()

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let area = NSScreen.screens.first?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1280, height: 800)
let window = NSWindow(
    contentRect: NSRect(x: area.maxX - size.width, y: area.maxY - size.height, width: size.width, height: size.height),
    styleMask: [.borderless], backing: .buffered, defer: false
)
window.title = "BTC Ticker end-to-end check: stand-in for World of Warcraft"
window.backgroundColor = NSColor(calibratedWhite: 0.12, alpha: 1)
window.isReleasedWhenClosed = false
window.orderFrontRegardless()

// Gone with the check that started it, or after three minutes at most, so a killed run never leaves it on screen.
let started = Date()
Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { _ in
    if getppid() != parent || Date().timeIntervalSince(started) > 180 { exit(0) }
}
app.run()

import AppKit
import SwiftUI
import TickerCore

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate, NSWindowDelegate {
    private var model: AppModel!
    private var overlay: OverlayController!
    private var statusItem: NSStatusItem!
    private let lockItem = NSMenuItem(title: "Lock Position", action: #selector(toggleLock), keyEquivalent: "")
    private var settingsWindow: NSWindow?
    /// The app that was frontmost when Settings opened, usually the game, which gets focus back when Settings closes.
    private var returnTo: NSRunningApplication?
    private var tasks: [Task<Void, Never>] = []

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
        NSApp.mainMenu = makeMainMenu()
        let environment = ProcessInfo.processInfo.environment
        let menu = makeMenu()
        model = AppModel(store: SettingsStore(environment: environment), frontmost: Frontmost(NSWorkspace.shared.frontmostApplication))
        overlay = OverlayController(model: model, menu: menu, stateFile: environment["BTCTICKER_STATE_FILE"].map { URL(fileURLWithPath: $0) })
        model.onChange = { [weak self] in self?.overlay.render() }
        overlay.render()

        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem.button?.image = NSImage(systemSymbolName: "bitcoinsign.circle", accessibilityDescription: "BTC Ticker")
        statusItem.menu = menu

        NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didActivateApplicationNotification, object: nil, queue: .main
        ) { [weak self] note in
            let frontmost = Frontmost(note.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication)
            MainActor.assumeIsolated { self?.model.send(.appActivated(frontmost)) }
        }
        NotificationCenter.default.addObserver(
            forName: NSApplication.didChangeScreenParametersNotification, object: nil, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.overlay.render() }
        }
        NSWorkspace.shared.notificationCenter.addObserver(forName: NSWorkspace.didWakeNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.model.woke()
                self?.overlay.render()
            }
        }

        tasks = [Task { await model.runFeed() }, Task { await model.runClock() }]
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        lockItem.title = model.overlay.settings.locked ? "Unlock Position" : "Lock Position"
    }

    private func makeMenu() -> NSMenu {
        let menu = NSMenu()
        menu.delegate = self
        menu.addItem(withTitle: "Settings…", action: #selector(openSettings), keyEquivalent: ",").target = self
        lockItem.target = self
        menu.addItem(lockItem)
        menu.addItem(.separator())
        menu.addItem(withTitle: "Quit BTC Ticker", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        return menu
    }

    /// Never shown, since the app has no Dock icon, but it is where Cmd-W and Cmd-Q in the Settings window are looked up.
    private func makeMainMenu() -> NSMenu {
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "Close Window", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        appMenu.addItem(withTitle: "Quit BTC Ticker", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        let menu = NSMenu()
        menu.addItem(withTitle: "BTC Ticker", action: nil, keyEquivalent: "").submenu = appMenu
        return menu
    }

    @objc private func toggleLock() {
        model.send(.toggleLock)
    }

    @objc private func openSettings() {
        if settingsWindow == nil {
            let window = NSWindow(contentViewController: NSHostingController(rootView: SettingsView(model: model)))
            window.title = "BTC Ticker Settings"
            window.styleMask = [.titled, .closable]
            window.collectionBehavior = [.moveToActiveSpace, .fullScreenAuxiliary]
            window.isReleasedWhenClosed = false
            window.delegate = self
            window.center()
            settingsWindow = window
        }
        if let frontmost = NSWorkspace.shared.frontmostApplication, frontmost != .current { returnTo = frontmost }
        NSApp.activate()
        settingsWindow?.makeKeyAndOrderFront(nil)
    }

    func windowWillClose(_ notification: Notification) {
        guard let previous = returnTo, !previous.isTerminated else {
            NSApp.deactivate()
            return
        }
        returnTo = nil
        NSApp.yieldActivation(to: previous)
        previous.activate(from: .current)
    }
}

let delegate = AppDelegate()
NSApplication.shared.delegate = delegate
NSApplication.shared.run()

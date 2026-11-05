import Testing
import TickerCore

/// The reducer's state plus the checker's own record of which app the OS last activated.
/// Keeping that record outside the reducer lets the visibility invariant compare two independent sources.
private struct World: Hashable {
    var state: OverlayState
    var frontmost: Frontmost
}

private struct Invariant: Sendable {
    let name: String
    let holds: @Sendable (_ before: World, _ event: OverlayEvent, _ after: World) -> Bool
}

private let dropPoints = [ScreenPoint(x: 10, y: 10), ScreenPoint(x: 900, y: 600)]

private func events(in state: OverlayState) -> [OverlayEvent] {
    var events: [OverlayEvent] = [.click, .dragBegan, .toggleLock]
    events += dropPoints.map { .dragEnded(at: $0) }
    events += Frontmost.allCases.map { .appActivated($0) }
    for locked in [false, true] {
        for onlyWhenWoWActive in [false, true] {
            var edited = state.settings
            edited.locked = locked
            edited.onlyWhenWoWActive = onlyWhenWoWActive
            events.append(.settingsEdited(edited))
        }
    }
    var reset = state.settings
    reset.position = nil
    events.append(.settingsEdited(reset))
    return events
}

private let invariants = [
    Invariant(name: "a locked overlay is never dragging") { _, _, after in
        !after.state.settings.locked || after.state.drag == .idle
    },
    Invariant(name: "a hidden overlay is never dragging") { _, _, after in
        after.state.isVisible || after.state.drag == .idle
    },
    Invariant(name: "with onlyWhenWoWActive on, visible iff WoW (or the ticker's own Settings) is frontmost") { _, _, after in
        let shouldShow = !after.state.settings.onlyWhenWoWActive || [.wow, .ticker].contains(after.frontmost)
        return after.state.isVisible == shouldShow
    },
    Invariant(name: "a click in collapsed mode never changes position") { before, event, after in
        event != .click || before.state.mode != .collapsed || after.state.settings.position == before.state.settings.position
    },
    Invariant(name: "a locked overlay moves only through a settings edit") { before, event, after in
        if case .settingsEdited = event { return true }
        return !before.state.settings.locked || after.state.settings.position == before.state.settings.position
    },
    Invariant(name: "a finished drag stores the drop point") { before, event, after in
        guard case .dragEnded(let point) = event, before.state.drag == .dragging else { return true }
        return after.state.settings.position == point && after.state.drag == .idle
    },
]

@Suite struct OverlayModelCheck {
    @Test func everyReachableTransitionKeepsTheInvariants() {
        var initial: [World] = []
        for locked in [false, true] {
            for onlyWhenWoWActive in [false, true] {
                for app in Frontmost.allCases {
                    var settings = Settings()
                    settings.locked = locked
                    settings.onlyWhenWoWActive = onlyWhenWoWActive
                    initial.append(World(state: OverlayState(settings: settings, frontmost: app), frontmost: app))
                }
            }
        }

        var queue = initial
        var seen = Set(initial)
        var parent: [World: (World, OverlayEvent)] = [:]
        var violations: [String: String] = [:]
        var transitions = 0

        func path(to world: World) -> [OverlayEvent] {
            var events: [OverlayEvent] = []
            var cursor = world
            while let (previous, event) = parent[cursor] {
                events.append(event)
                cursor = previous
            }
            return events.reversed()
        }

        var index = 0
        while index < queue.count {
            let world = queue[index]
            index += 1
            for event in events(in: world.state) {
                transitions += 1
                var next = World(state: OverlayState.reduce(world.state, event), frontmost: world.frontmost)
                if case .appActivated(let app) = event { next.frontmost = app }
                for invariant in invariants where violations[invariant.name] == nil && !invariant.holds(world, event, next) {
                    violations[invariant.name] = "\(path(to: world) + [event])"
                }
                if seen.insert(next).inserted {
                    parent[next] = (world, event)
                    queue.append(next)
                }
            }
        }

        for (name, trace) in violations.sorted(by: { $0.key < $1.key }) {
            Issue.record("invariant broken: \(name)\n  reached by: \(trace)")
        }
        #expect(queue.contains { $0.state.drag == .dragging }, "dragging must be reachable, or the drag invariants are vacuous")
        #expect(queue.contains { !$0.state.isVisible }, "hiding must be reachable, or the visibility invariant is vacuous")
        #expect(queue.contains { $0.state.settings.locked && $0.state.settings.position == dropPoints[1] })
        print("model check: \(seen.count) states, \(transitions) transitions, \(invariants.count) invariants")
    }

    @Test func dragThenDropMovesAnUnlockedOverlay() {
        var state = OverlayState(settings: Settings(), frontmost: .wow)
        for event in [OverlayEvent.dragBegan, .dragEnded(at: dropPoints[1])] {
            state = OverlayState.reduce(state, event)
        }
        #expect(state.settings.position == dropPoints[1])
    }

    @Test func lockingMidDragCancelsTheDrag() {
        var state = OverlayState(settings: Settings(), frontmost: .wow)
        for event in [OverlayEvent.dragBegan, .toggleLock, .dragEnded(at: dropPoints[1])] {
            state = OverlayState.reduce(state, event)
        }
        #expect(state.settings.locked)
        #expect(state.settings.position == nil)
    }
}

import Testing
import TickerCore

/// The reducer's state plus the checker's own records of which app the OS last activated and whether the
/// mouse has been down on a visible, locked overlay with only pointer moves since.
/// Keeping those records outside the reducer lets the invariants compare two independent sources.
private struct World: Hashable {
    var state: OverlayState
    var frontmost: Frontmost
    var lockedPress = false
}

private struct Invariant: Sendable {
    let name: String
    let holds: @Sendable (_ before: World, _ event: OverlayEvent, _ after: World) -> Bool
}

private let dropPoints = [ScreenPoint(x: 10, y: 10), ScreenPoint(x: 900, y: 600)]
private let targets: [PressTarget] = [.body, .outside] + Timeframe.allCases.map { .timeframe($0) }

private func events(in state: OverlayState) -> [OverlayEvent] {
    var events: [OverlayEvent] = [.pressed, .pointerMoved(distance: 1), .pointerMoved(distance: 50), .toggleLock]
    events += targets.flatMap { target in dropPoints.map { .released(over: target, tickerOrigin: $0) } }
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
    var retimed = state.settings
    retimed.timeframe = .m1
    events.append(.settingsEdited(retimed))
    return events
}

private let invariants = [
    Invariant(name: "a locked overlay is never dragging") { _, _, after in
        !after.state.settings.locked || after.state.drag != .dragging
    },
    Invariant(name: "a hidden overlay is never pressed or dragging") { _, _, after in
        after.state.isVisible || after.state.drag == .idle
    },
    Invariant(name: "with onlyWhenWoWActive on, visible iff WoW (or the ticker's own Settings) is frontmost") { _, _, after in
        let shouldShow = !after.state.settings.onlyWhenWoWActive || [.wow, .ticker].contains(after.frontmost)
        return after.state.isVisible == shouldShow
    },
    Invariant(name: "only a drag changes position, apart from a settings edit") { before, event, after in
        if case .settingsEdited = event { return true }
        if case .released = event, before.state.drag == .dragging { return true }
        return after.state.settings.position == before.state.settings.position
    },
    Invariant(name: "a click on the body expands to the last chosen timeframe, or collapses") { before, event, after in
        guard case .released(.body, _) = event, before.state.drag == .pressed else { return true }
        return after.state.mode == (before.state.mode == .collapsed ? .expanded(before.state.settings.timeframe) : .collapsed)
    },
    Invariant(name: "a locked press released over the body is a click, however far the pointer moved") { before, event, after in
        guard case .released(.body, _) = event, before.lockedPress else { return true }
        return after.state.mode != before.state.mode
    },
    Invariant(name: "releasing over a timeframe while collapsed, or outside the overlay, never changes the mode") { before, event, after in
        guard case .released(let target, _) = event else { return true }
        let inert = if case .timeframe = target { before.state.mode == .collapsed } else { target == .outside }
        return !inert || after.state.mode == before.state.mode
    },
    Invariant(name: "a release always ends the press") { _, event, after in
        guard case .released = event else { return true }
        return after.state.drag == .idle
    },
    Invariant(name: "expanded carries one of Timeframe.allCases, and it is the persisted choice") { _, _, after in
        guard case .expanded(let timeframe) = after.state.mode else { return true }
        return Timeframe.allCases.contains(timeframe) && timeframe == after.state.settings.timeframe
    },
    Invariant(name: "a click on a timeframe while expanded shows that timeframe") { before, event, after in
        guard case .released(.timeframe(let timeframe), _) = event, before.state.drag == .pressed, before.state.mode != .collapsed else { return true }
        return after.state.mode == .expanded(timeframe)
    },
    Invariant(name: "collapsing and re-expanding a visible overlay returns to the last chosen timeframe") { _, _, after in
        guard case .expanded = after.state.mode, after.state.isVisible else { return true }
        let collapsed = click(after.state)
        return collapsed.mode == .collapsed && click(collapsed).mode == after.state.mode
    },
    Invariant(name: "hiding and showing never change the mode") { before, event, after in
        let visibilityChanged = before.state.isVisible != after.state.isVisible
        let isActivation = if case .appActivated = event { true } else { false }
        return !(visibilityChanged || isActivation) || after.state.mode == before.state.mode
    },
    Invariant(name: "a locked overlay moves only through a settings edit") { before, event, after in
        if case .settingsEdited = event { return true }
        return !before.state.settings.locked || after.state.settings.position == before.state.settings.position
    },
    Invariant(name: "a finished drag stores the drop point and never changes the mode") { before, event, after in
        guard case .released(_, let point) = event, before.state.drag == .dragging else { return true }
        return after.state.settings.position == point && after.state.mode == before.state.mode
    },
]

private func click(_ state: OverlayState, on target: PressTarget = .body) -> OverlayState {
    [OverlayEvent.pressed, .released(over: target, tickerOrigin: dropPoints[0])].reduce(state, OverlayState.reduce)
}

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
                next.lockedPress = switch event {
                case .pressed: next.state.isVisible && next.state.settings.locked
                case .pointerMoved: world.lockedPress
                default: false
                }
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
        for timeframe in Timeframe.allCases {
            #expect(queue.contains { $0.state.mode == .expanded(timeframe) }, "expanded \(timeframe) must be reachable")
        }
        #expect(queue.contains { $0.state.mode != .collapsed && !$0.state.isVisible }, "hidden while expanded must be reachable")
        #expect(queue.contains { $0.state.mode != .collapsed && $0.state.settings.locked }, "clicks must expand a locked overlay")
        #expect(queue.contains { $0.lockedPress }, "a locked press must be reachable, or the locked-click invariant is vacuous")
        #expect(queue.contains { $0.state.mode == .collapsed && $0.state.settings.timeframe != Settings().timeframe },
                "collapsed with a non-default choice must be reachable, or the round trip only ever sees the default")
        print("model check: \(seen.count) states, \(transitions) transitions, \(invariants.count) invariants")
    }

    /// The pointer travel of one press, in points, against what the user meant by it.
    @Test(arguments: [false, true], [0.0, 2.9, 3, 40])
    func aPressIsADragOnlyWhenUnlockedAndPastTheThreshold(locked: Bool, travel: Double) {
        var settings = Settings()
        settings.locked = locked
        let start = OverlayState(settings: settings, frontmost: .wow)
        let end = [OverlayEvent.pressed, .pointerMoved(distance: travel / 2), .pointerMoved(distance: travel), .released(over: .body, tickerOrigin: dropPoints[1])]
            .reduce(start, OverlayState.reduce)
        if !locked && travel >= 3 {
            #expect(end.settings.position == dropPoints[1] && end.mode == .collapsed, "a drag moves and does not click")
        } else {
            #expect(end.settings.position == nil && end.mode == .expanded(.m15), "a click expands and does not move")
        }
        #expect(end.drag == .idle)
    }

    @Test func lockingMidDragCancelsTheGesture() {
        let events = [OverlayEvent.pressed, .pointerMoved(distance: 40), .toggleLock, .released(over: .body, tickerOrigin: dropPoints[1])]
        let end = events.reduce(OverlayState(settings: Settings(), frontmost: .wow), OverlayState.reduce)
        #expect(end.settings.locked)
        #expect(end.settings.position == nil && end.mode == .collapsed)
    }
}

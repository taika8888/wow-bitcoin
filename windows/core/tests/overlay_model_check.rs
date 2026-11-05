//! Port of OverlayModelCheckTests.swift: breadth-first over every state the reducer can reach under every event,
//! checking every invariant on every transition, plus the menu rules Windows adds.
//!
//! The checker takes the reducer as a parameter, and `MUTANTS` holds one deliberately broken reducer per invariant,
//! so each invariant is shown to fail, and adding an invariant without a mutant fails `every_invariant_has_a_mutant`.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use ticker_core::{
    DragPhase, Frontmost, MenuItem, OverlayEvent, OverlayMode, OverlayState, PressTarget,
    ScreenPoint, Settings, Timeframe, reduce,
};

type Reduce = fn(&OverlayState, &OverlayEvent) -> OverlayState;

/// The reducer's state plus the checker's own records, kept outside the reducer so invariants compare two sources.
#[derive(Clone, PartialEq, Eq, Hash)]
struct World {
    state: OverlayState,
    /// Which app the OS last activated, per the events, not per the reducer.
    frontmost: Frontmost,
    /// The button went down on a visible, locked overlay with only pointer moves since.
    locked_press: bool,
}

struct Invariant {
    name: &'static str,
    holds: fn(Reduce, &World, &OverlayEvent, &World) -> bool,
}

const DROP: [ScreenPoint; 2] = [
    ScreenPoint { x: 10.0, y: 10.0 },
    ScreenPoint { x: 900.0, y: 600.0 },
];

fn targets() -> Vec<PressTarget> {
    let mut t = vec![PressTarget::Body, PressTarget::Outside];
    t.extend(Timeframe::ALL.map(PressTarget::Timeframe));
    t.extend(MenuItem::ALL.map(PressTarget::Menu));
    t
}

fn events(state: &OverlayState) -> Vec<OverlayEvent> {
    use OverlayEvent::*;
    let mut e = vec![
        Pressed,
        PointerMoved { distance: 1.0 },
        PointerMoved { distance: 50.0 },
        ToggleLock,
        SecondaryClicked,
        PointerLeft,
    ];
    for over in targets() {
        e.extend(DROP.map(|ticker_origin| Released {
            over,
            ticker_origin,
        }));
    }
    e.extend(Frontmost::ALL.map(AppActivated));
    for locked in [false, true] {
        for only in [false, true] {
            let mut s = state.settings.clone();
            s.locked = locked;
            s.only_when_wow_active = only;
            e.push(SettingsEdited(s));
        }
    }
    let mut reset = state.settings.clone();
    reset.position = None;
    e.push(SettingsEdited(reset));
    let mut retimed = state.settings.clone();
    retimed.timeframe = Timeframe::M1;
    e.push(SettingsEdited(retimed));
    e
}

fn click(reduce: Reduce, state: &OverlayState, on: PressTarget) -> OverlayState {
    let pressed = reduce(state, &OverlayEvent::Pressed);
    reduce(
        &pressed,
        &OverlayEvent::Released {
            over: on,
            ticker_origin: DROP[0],
        },
    )
}

fn is_click_release(e: &OverlayEvent, before: &World, over: PressTarget) -> bool {
    matches!(e, OverlayEvent::Released { over: o, .. } if *o == over)
        && before.state.drag == DragPhase::Pressed
}

const LOCKED_NEVER_DRAGS: &str = "a locked overlay is never dragging";
const HIDDEN_IDLE: &str = "a hidden overlay is never pressed or dragging";
const VISIBLE_IFF_WOW: &str =
    "with onlyWhenWoWActive on, visible iff WoW (or the ticker's own Settings) is frontmost";
const ONLY_DRAG_MOVES: &str = "only a drag changes position, apart from a settings edit";
const BODY_CLICK: &str = "a click on the body expands to the last chosen timeframe, or collapses";
const LOCKED_CLICK: &str =
    "a locked press released over the body is a click, however far the pointer moved";
const INERT_RELEASE: &str =
    "releasing over a timeframe while collapsed, or outside the overlay, never changes the mode";
const RELEASE_ENDS_PRESS: &str = "a release always ends the press";
const EXPANDED_IS_PERSISTED: &str =
    "expanded carries one of Timeframe::ALL, and it is the persisted choice";
const TIMEFRAME_CLICK: &str = "a click on a timeframe while expanded shows that timeframe";
const ROUND_TRIP: &str =
    "collapsing and re-expanding a visible overlay returns to the last chosen timeframe";
const HIDE_SHOW_KEEPS_MODE: &str = "hiding and showing never change the mode";
const LOCKED_STAYS: &str = "a locked overlay moves only through a settings edit";
const DROP_STORES: &str = "a finished drag stores the drop point and never changes the mode";
const MENU_VISIBLE: &str = "an open menu is on a visible overlay";
const MENU_NO_DRAG: &str = "nothing drags while the menu is open";
const MENU_RELEASE: &str =
    "a release with the menu open closes it and never changes the mode or position";
const MENU_LOCK: &str = "Lock in the menu toggles the lock";

static INVARIANTS: &[Invariant] = &[
    Invariant {
        name: LOCKED_NEVER_DRAGS,
        holds: |_, _, _, a| !a.state.settings.locked || a.state.drag != DragPhase::Dragging,
    },
    Invariant {
        name: HIDDEN_IDLE,
        holds: |_, _, _, a| a.state.is_visible() || a.state.drag == DragPhase::Idle,
    },
    Invariant {
        name: VISIBLE_IFF_WOW,
        holds: |_, _, _, a| {
            let should = !a.state.settings.only_when_wow_active
                || matches!(a.frontmost, Frontmost::Wow | Frontmost::Ticker);
            a.state.is_visible() == should
        },
    },
    Invariant {
        name: ONLY_DRAG_MOVES,
        holds: |_, b, e, a| {
            matches!(e, OverlayEvent::SettingsEdited(_))
                || (matches!(e, OverlayEvent::Released { .. })
                    && b.state.drag == DragPhase::Dragging)
                || a.state.settings.position == b.state.settings.position
        },
    },
    Invariant {
        name: BODY_CLICK,
        holds: |_, b, e, a| {
            if !is_click_release(e, b, PressTarget::Body) || b.state.menu_open {
                return true;
            }
            a.state.mode
                == match b.state.mode {
                    OverlayMode::Collapsed => OverlayMode::Expanded(b.state.settings.timeframe),
                    OverlayMode::Expanded(_) => OverlayMode::Collapsed,
                }
        },
    },
    Invariant {
        name: LOCKED_CLICK,
        holds: |_, b, e, a| {
            let body = matches!(
                e,
                OverlayEvent::Released {
                    over: PressTarget::Body,
                    ..
                }
            );
            !body || !b.locked_press || b.state.menu_open || a.state.mode != b.state.mode
        },
    },
    Invariant {
        name: INERT_RELEASE,
        holds: |_, b, e, a| {
            let OverlayEvent::Released { over, .. } = e else {
                return true;
            };
            let inert = match over {
                PressTarget::Timeframe(_) => b.state.mode == OverlayMode::Collapsed,
                PressTarget::Outside => true,
                _ => false,
            };
            !inert || a.state.mode == b.state.mode
        },
    },
    Invariant {
        name: RELEASE_ENDS_PRESS,
        holds: |_, _, e, a| {
            !matches!(e, OverlayEvent::Released { .. }) || a.state.drag == DragPhase::Idle
        },
    },
    Invariant {
        name: EXPANDED_IS_PERSISTED,
        holds: |_, _, _, a| match a.state.mode {
            OverlayMode::Expanded(t) => {
                Timeframe::ALL.contains(&t) && t == a.state.settings.timeframe
            }
            OverlayMode::Collapsed => true,
        },
    },
    Invariant {
        name: TIMEFRAME_CLICK,
        holds: |_, b, e, a| {
            let OverlayEvent::Released {
                over: PressTarget::Timeframe(t),
                ..
            } = e
            else {
                return true;
            };
            b.state.drag != DragPhase::Pressed
                || b.state.menu_open
                || b.state.mode == OverlayMode::Collapsed
                || a.state.mode == OverlayMode::Expanded(*t)
        },
    },
    Invariant {
        name: ROUND_TRIP,
        holds: |r, _, _, a| {
            if !matches!(a.state.mode, OverlayMode::Expanded(_))
                || !a.state.is_visible()
                || a.state.menu_open
            {
                return true;
            }
            let collapsed = click(r, &a.state, PressTarget::Body);
            collapsed.mode == OverlayMode::Collapsed
                && click(r, &collapsed, PressTarget::Body).mode == a.state.mode
        },
    },
    Invariant {
        name: HIDE_SHOW_KEEPS_MODE,
        holds: |_, b, e, a| {
            let changed = b.state.is_visible() != a.state.is_visible()
                || matches!(e, OverlayEvent::AppActivated(_));
            !changed || a.state.mode == b.state.mode
        },
    },
    Invariant {
        name: LOCKED_STAYS,
        holds: |_, b, e, a| {
            matches!(e, OverlayEvent::SettingsEdited(_))
                || !b.state.settings.locked
                || a.state.settings.position == b.state.settings.position
        },
    },
    Invariant {
        name: DROP_STORES,
        holds: |_, b, e, a| {
            let OverlayEvent::Released { ticker_origin, .. } = e else {
                return true;
            };
            b.state.drag != DragPhase::Dragging
                || (a.state.settings.position == Some(*ticker_origin)
                    && a.state.mode == b.state.mode)
        },
    },
    Invariant {
        name: MENU_VISIBLE,
        holds: |_, _, _, a| !a.state.menu_open || a.state.is_visible(),
    },
    Invariant {
        name: MENU_NO_DRAG,
        holds: |_, _, _, a| !a.state.menu_open || a.state.drag != DragPhase::Dragging,
    },
    Invariant {
        name: MENU_RELEASE,
        holds: |_, b, e, a| {
            if !matches!(e, OverlayEvent::Released { .. }) || !b.state.menu_open {
                return true;
            }
            !a.state.menu_open
                && a.state.mode == b.state.mode
                && a.state.settings.position == b.state.settings.position
        },
    },
    Invariant {
        name: MENU_LOCK,
        holds: |_, b, e, a| {
            !is_click_release(e, b, PressTarget::Menu(MenuItem::Lock))
                || !b.state.menu_open
                || a.state.settings.locked != b.state.settings.locked
        },
    },
];

struct Exploration {
    reached: Vec<World>,
    transitions: usize,
    /// Invariant name -> the shortest event trace that broke it (BFS order makes it shortest).
    violations: BTreeMap<&'static str, Vec<OverlayEvent>>,
}

fn explore(reduce: Reduce) -> Exploration {
    let mut initial = Vec::new();
    for locked in [false, true] {
        for only in [false, true] {
            for app in Frontmost::ALL {
                let settings = Settings {
                    locked,
                    only_when_wow_active: only,
                    ..Settings::default()
                };
                initial.push(World {
                    state: OverlayState::new(settings, app),
                    frontmost: app,
                    locked_press: false,
                });
            }
        }
    }
    let mut queue: VecDeque<World> = initial.iter().cloned().collect();
    let mut seen: HashSet<World> = initial.into_iter().collect();
    let mut parent: HashMap<World, (World, OverlayEvent)> = HashMap::new();
    let mut reached = Vec::new();
    let mut violations = BTreeMap::new();
    let mut transitions = 0;
    let path = |parent: &HashMap<World, (World, OverlayEvent)>, mut w: World| {
        let mut trace = Vec::new();
        while let Some((prev, e)) = parent.get(&w) {
            trace.push(e.clone());
            w = prev.clone();
        }
        trace.reverse();
        trace
    };
    while let Some(world) = queue.pop_front() {
        for event in events(&world.state) {
            transitions += 1;
            let state = reduce(&world.state, &event);
            let frontmost = if let OverlayEvent::AppActivated(app) = event {
                app
            } else {
                world.frontmost
            };
            let locked_press = match event {
                OverlayEvent::Pressed => state.is_visible() && state.settings.locked,
                OverlayEvent::PointerMoved { .. } => world.locked_press,
                _ => false,
            };
            let next = World {
                state,
                frontmost,
                locked_press,
            };
            for inv in INVARIANTS {
                if !violations.contains_key(inv.name) && !(inv.holds)(reduce, &world, &event, &next)
                {
                    let mut trace = path(&parent, world.clone());
                    trace.push(event.clone());
                    violations.insert(inv.name, trace);
                }
            }
            if seen.insert(next.clone()) {
                parent.insert(next.clone(), (world.clone(), event.clone()));
                queue.push_back(next);
            }
        }
        reached.push(world);
    }
    Exploration {
        reached,
        transitions,
        violations,
    }
}

#[test]
fn every_reachable_transition_keeps_the_invariants() {
    let x = explore(reduce);
    for (name, trace) in &x.violations {
        eprintln!("invariant broken: {name}\n  reached by: {trace:?}");
    }
    assert!(
        x.violations.is_empty(),
        "the real reducer broke an invariant"
    );
    let any = |p: &dyn Fn(&World) -> bool| x.reached.iter().any(p);
    assert!(
        any(&|w| w.state.drag == DragPhase::Dragging),
        "dragging must be reachable"
    );
    assert!(any(&|w| !w.state.is_visible()), "hiding must be reachable");
    assert!(
        any(&|w| w.state.settings.locked && w.state.settings.position == Some(DROP[1])),
        "a dropped overlay must be lockable, or the locked-move invariant only sees the default spot"
    );
    for t in Timeframe::ALL {
        assert!(
            any(&|w| w.state.mode == OverlayMode::Expanded(t)),
            "expanded {t:?} must be reachable"
        );
    }
    assert!(
        any(&|w| w.state.mode != OverlayMode::Collapsed && !w.state.is_visible()),
        "hidden while expanded must be reachable"
    );
    assert!(
        any(&|w| w.state.mode != OverlayMode::Collapsed && w.state.settings.locked),
        "clicks must expand a locked overlay"
    );
    assert!(
        any(&|w| w.locked_press),
        "a locked press must be reachable, or the locked-click invariant is vacuous"
    );
    assert!(
        any(&|w| w.state.mode == OverlayMode::Collapsed
            && w.state.settings.timeframe != Settings::default().timeframe),
        "collapsed with a non-default choice must be reachable, or the round trip only ever sees the default"
    );
    assert!(
        any(&|w| w.state.menu_open
            && w.state.mode != OverlayMode::Collapsed
            && w.state.settings.locked),
        "the menu must open over a locked chart, or the menu invariants only see the collapsed ticker"
    );
    println!(
        "model check: {} states, {} transitions, {} invariants",
        x.reached.len(),
        x.transitions,
        INVARIANTS.len()
    );
}

/// One deliberately broken reducer per invariant: the real one with a single behavior bent.
struct Mutant {
    breaks: &'static str,
    /// Invariants the same bug must also break, because together they imply `breaks`.
    also_breaks: &'static [&'static str],
    reduce: Reduce,
}

fn press_released(s: &OverlayState, e: &OverlayEvent, over: PressTarget) -> bool {
    matches!(e, OverlayEvent::Released { over: o, .. } if *o == over)
        && s.drag == DragPhase::Pressed
        && !s.menu_open
}

static MUTANTS: &[Mutant] = &[
    Mutant {
        breaks: LOCKED_NEVER_DRAGS,
        // Releasing a locked drag must store the drop point and must not move a locked overlay.
        also_breaks: &[LOCKED_STAYS],
        reduce: |s, e| {
            let mut n = reduce(s, e);
            if matches!(e, OverlayEvent::ToggleLock) && s.drag == DragPhase::Dragging {
                n.drag = DragPhase::Dragging;
            }
            n
        },
    },
    Mutant {
        breaks: HIDDEN_IDLE,
        also_breaks: &[],
        reduce: |s, e| {
            let mut n = reduce(s, e);
            if *e == OverlayEvent::Pressed {
                n.drag = DragPhase::Pressed;
            }
            n
        },
    },
    Mutant {
        breaks: VISIBLE_IFF_WOW,
        also_breaks: &[],
        reduce: |s, e| {
            let mut n = reduce(s, e);
            if matches!(e, OverlayEvent::AppActivated(_)) {
                n.frontmost = s.frontmost;
            }
            n
        },
    },
    Mutant {
        breaks: ONLY_DRAG_MOVES,
        also_breaks: &[],
        reduce: |s, e| {
            let mut n = reduce(s, e);
            if let OverlayEvent::Released { ticker_origin, .. } = e
                && s.drag == DragPhase::Pressed
                && !s.settings.locked
                && !s.menu_open
            {
                n.settings.position = Some(*ticker_origin);
            }
            n
        },
    },
    Mutant {
        breaks: BODY_CLICK,
        also_breaks: &[],
        reduce: |s, e| {
            let mut n = reduce(s, e);
            if press_released(s, e, PressTarget::Body)
                && matches!(e, OverlayEvent::Released { ticker_origin, .. } if *ticker_origin == DROP[1])
                && s.mode == OverlayMode::Collapsed
                && s.settings.timeframe != Timeframe::M1
            {
                n.settings.timeframe = Timeframe::M1;
                n.mode = OverlayMode::Expanded(Timeframe::M1);
            }
            n
        },
    },
    Mutant {
        breaks: LOCKED_CLICK,
        also_breaks: &[],
        reduce: |s, e| {
            let mut n = reduce(s, e);
            if matches!(e, OverlayEvent::PointerMoved { distance } if *distance >= 3.0)
                && s.settings.locked
            {
                n.drag = DragPhase::Idle;
            }
            n
        },
    },
    Mutant {
        breaks: INERT_RELEASE,
        also_breaks: &[],
        reduce: |s, e| match e {
            OverlayEvent::Released {
                over: PressTarget::Outside,
                ticker_origin,
            } => reduce(
                s,
                &OverlayEvent::Released {
                    over: PressTarget::Body,
                    ticker_origin: *ticker_origin,
                },
            ),
            _ => reduce(s, e),
        },
    },
    Mutant {
        breaks: RELEASE_ENDS_PRESS,
        also_breaks: &[],
        reduce: |s, e| {
            let mut n = reduce(s, e);
            if press_released(s, e, PressTarget::Outside) {
                n.drag = DragPhase::Pressed;
            }
            n
        },
    },
    Mutant {
        breaks: EXPANDED_IS_PERSISTED,
        // A visible expanded state off the persisted choice cannot collapse and come back to itself.
        also_breaks: &[ROUND_TRIP],
        reduce: |s, e| {
            let mut n = reduce(s, e);
            let picked = Timeframe::ALL
                .into_iter()
                .any(|t| press_released(s, e, PressTarget::Timeframe(t)));
            if !picked
                && matches!(s.mode, OverlayMode::Expanded(_))
                && matches!(n.mode, OverlayMode::Expanded(_))
            {
                n.mode = s.mode;
            }
            n
        },
    },
    Mutant {
        breaks: TIMEFRAME_CLICK,
        also_breaks: &[],
        reduce: |s, e| {
            let n = reduce(s, e);
            let on_timeframe = Timeframe::ALL
                .into_iter()
                .any(|t| press_released(s, e, PressTarget::Timeframe(t)));
            if on_timeframe && s.mode != OverlayMode::Collapsed {
                return OverlayState {
                    drag: DragPhase::Idle,
                    ..s.clone()
                };
            }
            n
        },
    },
    Mutant {
        breaks: ROUND_TRIP,
        also_breaks: &[],
        reduce: |s, e| {
            let mut n = reduce(s, e);
            if n.mode == OverlayMode::Collapsed
                && s.mode != OverlayMode::Collapsed
                && matches!(e, OverlayEvent::Released { .. })
            {
                n.settings.timeframe = Timeframe::M15;
            }
            n
        },
    },
    Mutant {
        breaks: HIDE_SHOW_KEEPS_MODE,
        also_breaks: &[],
        reduce: |s, e| {
            let mut n = reduce(s, e);
            if !n.is_visible() {
                n.mode = OverlayMode::Collapsed;
            }
            n
        },
    },
    Mutant {
        breaks: LOCKED_STAYS,
        // A locked overlay never drags, so moving it on a release is a move without a drag.
        also_breaks: &[ONLY_DRAG_MOVES],
        reduce: |s, e| {
            let mut n = reduce(s, e);
            if let OverlayEvent::Released { ticker_origin, .. } = e
                && s.settings.locked
                && !s.menu_open
            {
                n.settings.position = Some(*ticker_origin);
            }
            n
        },
    },
    Mutant {
        breaks: DROP_STORES,
        also_breaks: &[],
        reduce: |s, e| {
            let mut n = reduce(s, e);
            if matches!(
                e,
                OverlayEvent::Released {
                    over: PressTarget::Body,
                    ..
                }
            ) && s.drag == DragPhase::Dragging
            {
                n.mode = OverlayMode::Collapsed;
            }
            n
        },
    },
    Mutant {
        breaks: MENU_VISIBLE,
        also_breaks: &[],
        reduce: |s, e| {
            let mut n = reduce(s, e);
            if matches!(
                e,
                OverlayEvent::AppActivated(_) | OverlayEvent::SettingsEdited(_)
            ) {
                n.menu_open = s.menu_open;
            }
            n
        },
    },
    Mutant {
        breaks: MENU_NO_DRAG,
        // Releasing a drag with the menu open must both store the drop point and leave the position alone.
        also_breaks: &[DROP_STORES],
        reduce: |s, e| {
            let mut n = reduce(s, e);
            if matches!(e, OverlayEvent::PointerMoved { distance } if *distance >= 3.0)
                && s.menu_open
                && s.drag == DragPhase::Pressed
                && !s.settings.locked
            {
                n.drag = DragPhase::Dragging;
            }
            n
        },
    },
    Mutant {
        breaks: MENU_RELEASE,
        also_breaks: &[],
        reduce: |s, e| {
            let mut n = reduce(s, e);
            if matches!(
                e,
                OverlayEvent::Released {
                    over: PressTarget::Body,
                    ..
                }
            ) && s.menu_open
            {
                n.menu_open = true;
            }
            n
        },
    },
    Mutant {
        breaks: MENU_LOCK,
        also_breaks: &[],
        reduce: |s, e| {
            let mut n = reduce(s, e);
            if matches!(
                e,
                OverlayEvent::Released {
                    over: PressTarget::Menu(MenuItem::Lock),
                    ..
                }
            ) {
                n.settings.locked = s.settings.locked;
            }
            n
        },
    },
];

#[test]
fn every_invariant_has_a_mutant() {
    let names: HashSet<_> = INVARIANTS.iter().map(|i| i.name).collect();
    let killed: HashSet<_> = MUTANTS.iter().map(|m| m.breaks).collect();
    assert_eq!(
        names, killed,
        "add a mutant for each new invariant, and name only real invariants"
    );
}

#[test]
fn every_mutant_breaks_exactly_its_invariant() {
    let wrong: Vec<String> = MUTANTS
        .iter()
        .filter_map(|mutant| {
            let broken: BTreeSet<&str> = explore(mutant.reduce).violations.into_keys().collect();
            let expected: BTreeSet<&str> = std::iter::once(mutant.breaks)
                .chain(mutant.also_breaks.iter().copied())
                .collect();
            (broken != expected).then(|| {
                format!(
                    "mutant for '{}' broke {broken:?}, expected {expected:?}",
                    mutant.breaks
                )
            })
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_press_is_a_drag_only_when_unlocked_and_past_the_threshold() {
    for locked in [false, true] {
        for travel in [0.0, 2.9, 3.0, 40.0] {
            let start = OverlayState::new(
                Settings {
                    locked,
                    ..Settings::default()
                },
                Frontmost::Wow,
            );
            let end = [
                OverlayEvent::Pressed,
                OverlayEvent::PointerMoved {
                    distance: travel / 2.0,
                },
                OverlayEvent::PointerMoved { distance: travel },
                OverlayEvent::Released {
                    over: PressTarget::Body,
                    ticker_origin: DROP[1],
                },
            ]
            .iter()
            .fold(start, |s, e| reduce(&s, e));
            if !locked && travel >= 3.0 {
                assert_eq!(
                    (end.settings.position, end.mode),
                    (Some(DROP[1]), OverlayMode::Collapsed),
                    "a drag moves and does not click (travel {travel})"
                );
            } else {
                assert_eq!(
                    (end.settings.position, end.mode),
                    (None, OverlayMode::Expanded(Timeframe::M15)),
                    "a click expands and does not move (locked {locked}, travel {travel})"
                );
            }
            assert_eq!(end.drag, DragPhase::Idle);
        }
    }
}

#[test]
fn locking_mid_drag_cancels_the_gesture() {
    let end = [
        OverlayEvent::Pressed,
        OverlayEvent::PointerMoved { distance: 40.0 },
        OverlayEvent::ToggleLock,
        OverlayEvent::Released {
            over: PressTarget::Body,
            ticker_origin: DROP[1],
        },
    ]
    .iter()
    .fold(
        OverlayState::new(Settings::default(), Frontmost::Wow),
        |s, e| reduce(&s, e),
    );
    assert!(end.settings.locked);
    assert_eq!(
        (end.settings.position, end.mode),
        (None, OverlayMode::Collapsed)
    );
}

#[test]
fn wow_executables_are_recognized_by_file_name() {
    for (path, is_self, expected) in [
        (
            Some(r"C:\Program Files (x86)\World of Warcraft\_retail_\Wow.exe"),
            false,
            Frontmost::Wow,
        ),
        (
            Some(r"D:\Games\WoW\_classic_\WOWCLASSIC.EXE"),
            false,
            Frontmost::Wow,
        ),
        (Some(r"C:\Windows\explorer.exe"), false, Frontmost::Other),
        (Some(r"C:\Tools\NotWow.exe"), false, Frontmost::Other),
        (None, false, Frontmost::Other),
        (Some(r"C:\Apps\btc-ticker.exe"), true, Frontmost::Ticker),
    ] {
        assert_eq!(Frontmost::classify(path, is_self), expected, "{path:?}");
    }
}

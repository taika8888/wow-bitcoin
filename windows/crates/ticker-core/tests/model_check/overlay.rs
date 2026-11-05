//! Every state `OverlayState::reduce` can reach, under every event, port of OverlayModelCheckTests.swift.

use ticker_core::{
    DragPhase, Frontmost, OverlayEvent, OverlayMode, OverlayState, PressTarget, ScreenPoint, Settings, Timeframe,
};

use crate::engine::{Model, explore};
use crate::words::{frontmost, overlay_event, settings_words};

const DROP_POINTS: [ScreenPoint; 2] = [ScreenPoint { x: 10.0, y: 10.0 }, ScreenPoint { x: 900.0, y: 600.0 }];

/// The reducer's state plus the checker's own records of which app the OS last activated and whether the mouse has
/// been down on a visible, locked overlay with only pointer moves since. Keeping those outside the reducer lets the
/// invariants compare two independent sources.
#[derive(Clone, Debug)]
pub struct World {
    state: OverlayState,
    frontmost: Frontmost,
    locked_press: bool,
}

type Invariant = (&'static str, fn(&World, &OverlayEvent, &World) -> bool);

const INVARIANTS: [Invariant; 14] = [
    ("a locked overlay is never dragging", |_, _, after| {
        !after.state.settings().locked || after.state.drag() != DragPhase::Dragging
    }),
    ("a hidden overlay is never pressed or dragging", |_, _, after| {
        after.state.is_visible() || after.state.drag() == DragPhase::Idle
    }),
    ("with onlyWhenWoWActive on, visible iff WoW (or the ticker's own settings) is frontmost", |_, _, after| {
        let should_show = !after.state.settings().only_when_wow_active || after.frontmost != Frontmost::Other;
        after.state.is_visible() == should_show
    }),
    ("only a drag changes position, apart from a settings edit", |before, event, after| match event {
        OverlayEvent::SettingsEdited(_) => true,
        OverlayEvent::Released { .. } if before.state.drag() == DragPhase::Dragging => true,
        _ => after.state.settings().position == before.state.settings().position,
    }),
    ("a click on the body expands to the last chosen timeframe, or collapses", |before, event, after| {
        let OverlayEvent::Released { over: PressTarget::Body, .. } = event else { return true };
        if before.state.drag() != DragPhase::Pressed {
            return true;
        }
        let expected = match before.state.mode() {
            OverlayMode::Collapsed => OverlayMode::Expanded(before.state.settings().timeframe),
            OverlayMode::Expanded(_) => OverlayMode::Collapsed,
        };
        after.state.mode() == expected
    }),
    ("a locked press released over the body is a click, however far the pointer moved", |before, event, after| {
        let OverlayEvent::Released { over: PressTarget::Body, .. } = event else { return true };
        !before.locked_press || after.state.mode() != before.state.mode()
    }),
    (
        "releasing over a timeframe while collapsed, or outside the overlay, never changes the mode",
        |before, event, after| {
            let OverlayEvent::Released { over, .. } = event else { return true };
            let inert = match over {
                PressTarget::Timeframe(_) => before.state.mode() == OverlayMode::Collapsed,
                PressTarget::Outside => true,
                PressTarget::Body => false,
            };
            !inert || after.state.mode() == before.state.mode()
        },
    ),
    ("a release always ends the press", |_, event, after| {
        !matches!(event, OverlayEvent::Released { .. }) || after.state.drag() == DragPhase::Idle
    }),
    ("expanded carries one of Timeframe::ALL, and it is the persisted choice", |_, _, after| {
        match after.state.mode() {
            OverlayMode::Expanded(timeframe) => {
                Timeframe::ALL.contains(&timeframe) && timeframe == after.state.settings().timeframe
            }
            OverlayMode::Collapsed => true,
        }
    }),
    ("a click on a timeframe while expanded shows that timeframe", |before, event, after| {
        let OverlayEvent::Released { over: PressTarget::Timeframe(timeframe), .. } = event else { return true };
        before.state.drag() != DragPhase::Pressed
            || before.state.mode() == OverlayMode::Collapsed
            || after.state.mode() == OverlayMode::Expanded(*timeframe)
    }),
    ("collapsing and re-expanding a visible overlay returns to the last chosen timeframe", |_, _, after| {
        if after.state.mode() == OverlayMode::Collapsed || !after.state.is_visible() {
            return true;
        }
        let collapsed = click(&after.state);
        collapsed.mode() == OverlayMode::Collapsed && click(&collapsed).mode() == after.state.mode()
    }),
    ("hiding and showing never change the mode", |before, event, after| {
        let visibility_changed = before.state.is_visible() != after.state.is_visible();
        let activation = matches!(event, OverlayEvent::AppActivated(_));
        !(visibility_changed || activation) || after.state.mode() == before.state.mode()
    }),
    ("a locked overlay moves only through a settings edit", |before, event, after| {
        matches!(event, OverlayEvent::SettingsEdited(_))
            || !before.state.settings().locked
            || after.state.settings().position == before.state.settings().position
    }),
    ("a finished drag stores the drop point and never changes the mode", |before, event, after| {
        let OverlayEvent::Released { ticker_origin, .. } = event else { return true };
        before.state.drag() != DragPhase::Dragging
            || (after.state.settings().position == Some(*ticker_origin) && after.state.mode() == before.state.mode())
    }),
];

fn click(state: &OverlayState) -> OverlayState {
    state
        .reduce(OverlayEvent::Pressed)
        .reduce(OverlayEvent::Released { over: PressTarget::Body, ticker_origin: DROP_POINTS[0] })
}

pub fn events(settings: &Settings) -> Vec<OverlayEvent> {
    let mut events = vec![
        OverlayEvent::Pressed,
        OverlayEvent::PointerMoved { distance: 1.0 },
        OverlayEvent::PointerMoved { distance: 50.0 },
        OverlayEvent::ToggleLock,
    ];
    let targets =
        [PressTarget::Body, PressTarget::Outside].into_iter().chain(Timeframe::ALL.map(PressTarget::Timeframe));
    for over in targets {
        events.extend(DROP_POINTS.map(|ticker_origin| OverlayEvent::Released { over, ticker_origin }));
    }
    events.extend(Frontmost::ALL.map(OverlayEvent::AppActivated));
    for locked in [false, true] {
        for only_when_wow_active in [false, true] {
            events.push(OverlayEvent::SettingsEdited(Settings { locked, only_when_wow_active, ..*settings }));
        }
    }
    events.push(OverlayEvent::SettingsEdited(Settings { position: None, ..*settings }));
    events.push(OverlayEvent::SettingsEdited(Settings { timeframe: Timeframe::M1, ..*settings }));
    events
}

struct Reducer;

impl Model for Reducer {
    type World = World;
    type Step = OverlayEvent;

    fn initial(&self) -> Vec<World> {
        let mut worlds = Vec::new();
        for locked in [false, true] {
            for only_when_wow_active in [false, true] {
                for app in Frontmost::ALL {
                    let settings = Settings { locked, only_when_wow_active, ..Settings::default() };
                    worlds.push(World { state: OverlayState::new(settings, app), frontmost: app, locked_press: false });
                }
            }
        }
        worlds
    }

    fn steps(&self, world: &World) -> Vec<OverlayEvent> {
        events(world.state.settings())
    }

    fn describe(&self, event: &OverlayEvent) -> String {
        overlay_event(event)
    }

    fn describe_initial(&self, world: &World) -> String {
        format!("{} in the foreground, {}", frontmost(world.frontmost), settings_words(world.state.settings()))
    }

    fn apply(&self, world: &World, event: &OverlayEvent) -> (World, Vec<&'static str>) {
        let state = world.state.reduce(*event);
        let frontmost = if let OverlayEvent::AppActivated(app) = event { *app } else { world.frontmost };
        let locked_press = match event {
            OverlayEvent::Pressed => state.is_visible() && state.settings().locked,
            OverlayEvent::PointerMoved { .. } => world.locked_press,
            _ => false,
        };
        let after = World { state, frontmost, locked_press };
        let broken =
            INVARIANTS.iter().filter(|(_, holds)| !holds(world, event, &after)).map(|(name, _)| *name).collect();
        (after, broken)
    }
}

#[test]
fn every_reachable_reducer_transition_keeps_the_invariants() {
    let mut reached = Vec::new();
    let mut locked_press = false;
    let report = explore("overlay", &Reducer, None, |world| {
        reached.push(world.state);
        locked_press |= world.locked_press;
    });
    println!("{} ({} invariants)", report.summary(), INVARIANTS.len());
    assert!(report.violations.is_empty(), "{}", report.failures());
    assert_eq!(report.unexpanded, 0, "the reducer's state space is finite, so the check must exhaust it");

    let any = |predicate: fn(&OverlayState) -> bool| reached.iter().any(predicate);
    assert!(any(|s| s.drag() == DragPhase::Dragging), "dragging must be reachable, or the drag invariants are vacuous");
    assert!(any(|s| !s.is_visible()), "hiding must be reachable, or the visibility invariant is vacuous");
    assert!(any(|s| s.settings().locked && s.settings().position == Some(DROP_POINTS[1])));
    for timeframe in Timeframe::ALL {
        assert!(
            reached.iter().any(|s| s.mode() == OverlayMode::Expanded(timeframe)),
            "expanded {timeframe:?} must be reachable"
        );
    }
    assert!(any(|s| s.mode() != OverlayMode::Collapsed && !s.is_visible()), "hidden while expanded must be reachable");
    assert!(any(|s| s.mode() != OverlayMode::Collapsed && s.settings().locked), "clicks must expand a locked overlay");
    assert!(
        any(|s| s.mode() == OverlayMode::Collapsed && s.settings().timeframe != Settings::default().timeframe),
        "collapsed with a non-default choice must be reachable, or the round trip only ever sees the default"
    );
    assert!(locked_press, "a locked press must be reachable, or the locked-click invariant is vacuous");
}

#[test]
fn a_press_is_a_drag_only_when_unlocked_and_past_the_threshold() {
    for locked in [false, true] {
        for travel in [0.0, 2.9, 3.0, 40.0] {
            let start = OverlayState::new(Settings { locked, ..Settings::default() }, Frontmost::Wow);
            let end = [
                OverlayEvent::Pressed,
                OverlayEvent::PointerMoved { distance: travel / 2.0 },
                OverlayEvent::PointerMoved { distance: travel },
                OverlayEvent::Released { over: PressTarget::Body, ticker_origin: DROP_POINTS[1] },
            ]
            .into_iter()
            .fold(start, |state, event| state.reduce(event));
            if !locked && travel >= 3.0 {
                assert!(
                    end.settings().position == Some(DROP_POINTS[1]) && end.mode() == OverlayMode::Collapsed,
                    "a drag moves and does not click"
                );
            } else {
                assert!(
                    end.settings().position.is_none() && end.mode() == OverlayMode::Expanded(Timeframe::M15),
                    "locked {locked}, travel {travel}: a click expands and does not move"
                );
            }
            assert_eq!(end.drag(), DragPhase::Idle);
        }
    }
}

#[test]
fn locking_mid_drag_cancels_the_gesture() {
    let end = [
        OverlayEvent::Pressed,
        OverlayEvent::PointerMoved { distance: 40.0 },
        OverlayEvent::ToggleLock,
        OverlayEvent::Released { over: PressTarget::Body, ticker_origin: DROP_POINTS[1] },
    ]
    .into_iter()
    .fold(OverlayState::new(Settings::default(), Frontmost::Wow), |state, event| state.reduce(event));
    assert!(end.settings().locked);
    assert!(end.settings().position.is_none() && end.mode() == OverlayMode::Collapsed);
}

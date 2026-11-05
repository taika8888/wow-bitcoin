//! Overlay.swift plus `menu_open`. On Windows TrackPopupMenu needs its owner in the foreground, which would take
//! focus from the game, so the menu is drawn inside the overlay and its rules live in this reducer.
use crate::market::Timeframe;
use crate::settings::{ScreenPoint, Settings};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OverlayMode {
    Collapsed,
    Expanded(Timeframe),
}

impl OverlayMode {
    pub fn name(self) -> &'static str {
        match self {
            OverlayMode::Collapsed => "collapsed",
            OverlayMode::Expanded(_) => "expanded",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Frontmost {
    Wow,
    /// This app, which is foreground only while its Settings window is in use.
    Ticker,
    Other,
}

impl Frontmost {
    pub const ALL: [Frontmost; 3] = [Frontmost::Wow, Frontmost::Ticker, Frontmost::Other];
    /// Retail, Classic, PTR and beta clients, matched case-insensitively against the file name.
    pub const WOW_EXECUTABLES: &[&str] = &[
        "Wow.exe",
        "WowT.exe",
        "WowB.exe",
        "WowClassic.exe",
        "WowClassicT.exe",
        "WowClassicB.exe",
    ];

    /// `image_path` from QueryFullProcessImageNameW of the foreground window's process; `is_self` when that is us.
    pub fn classify(image_path: Option<&str>, is_self: bool) -> Frontmost {
        if is_self {
            return Frontmost::Ticker;
        }
        let file = image_path.map(|path| path.rsplit(['\\', '/']).next().unwrap_or(path));
        match file {
            Some(file)
                if Self::WOW_EXECUTABLES
                    .iter()
                    .any(|exe| exe.eq_ignore_ascii_case(file)) =>
            {
                Frontmost::Wow
            }
            _ => Frontmost::Other,
        }
    }
}

/// A press becomes a drag only once the pointer travels `DRAG_THRESHOLD` on an unlocked overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DragPhase {
    Idle,
    Pressed,
    Dragging,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MenuItem {
    Settings,
    Lock,
    Quit,
}

impl MenuItem {
    pub const ALL: [MenuItem; 3] = [MenuItem::Settings, MenuItem::Lock, MenuItem::Quit];

    pub fn name(self) -> &'static str {
        match self {
            MenuItem::Settings => "settings",
            MenuItem::Lock => "lock",
            MenuItem::Quit => "quit",
        }
    }
}

/// What is under the pointer when the button comes up, from `Frame::hit` over the same rects that were drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PressTarget {
    /// Anywhere on the overlay outside the timeframe controls and the menu.
    Body,
    Timeframe(Timeframe),
    Menu(MenuItem),
    Outside,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OverlayState {
    pub settings: Settings,
    pub frontmost: Frontmost,
    pub mode: OverlayMode,
    pub drag: DragPhase,
    pub menu_open: bool,
}

impl OverlayState {
    pub const DRAG_THRESHOLD: f64 = 3.0;

    pub fn new(settings: Settings, frontmost: Frontmost) -> OverlayState {
        OverlayState {
            settings,
            frontmost,
            mode: OverlayMode::Collapsed,
            drag: DragPhase::Idle,
            menu_open: false,
        }
    }

    /// The ticker counts as "WoW active" too, so the overlay stays up while its own Settings window is open.
    pub fn is_visible(&self) -> bool {
        !self.settings.only_when_wow_active || self.frontmost != Frontmost::Other
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum OverlayEvent {
    Pressed,
    /// The pointer is `distance` px from where the button went down.
    PointerMoved {
        distance: f64,
    },
    /// A drag drops the ticker line at `ticker_origin`. Any other press is a click: on the body it toggles between
    /// the ticker and the chart, on a timeframe control it selects that timeframe.
    Released {
        over: PressTarget,
        ticker_origin: ScreenPoint,
    },
    /// Right button up over the overlay: opens the menu, or closes it if open.
    SecondaryClicked,
    /// WM_MOUSELEAVE. Closes the menu, since a click elsewhere in the game never reaches us.
    PointerLeft,
    ToggleLock,
    AppActivated(Frontmost),
    SettingsEdited(Settings),
}

/// Overlay.swift's transitions, plus the menu: a right click toggles it while no press is under way, nothing drags
/// while it is open, and any release closes it without clicking the chart. A press released over Lock toggles the
/// lock; Settings and Quit are effects `App` emits for the same release.
pub fn reduce(state: &OverlayState, event: &OverlayEvent) -> OverlayState {
    let mut next = state.clone();
    match event {
        OverlayEvent::Pressed => next.drag = DragPhase::Pressed,
        OverlayEvent::PointerMoved { distance } => {
            if state.drag == DragPhase::Pressed
                && !state.settings.locked
                && !state.menu_open
                && *distance >= OverlayState::DRAG_THRESHOLD
            {
                next.drag = DragPhase::Dragging;
            }
        }
        OverlayEvent::Released {
            over,
            ticker_origin,
        } => {
            next.drag = DragPhase::Idle;
            if state.menu_open {
                next.menu_open = false;
                if state.drag == DragPhase::Pressed && *over == PressTarget::Menu(MenuItem::Lock) {
                    next.settings.locked = !state.settings.locked;
                }
            } else {
                match (state.drag, over, state.mode) {
                    (DragPhase::Dragging, _, _) => next.settings.position = Some(*ticker_origin),
                    (DragPhase::Pressed, PressTarget::Body, OverlayMode::Collapsed) => {
                        next.mode = OverlayMode::Expanded(state.settings.timeframe)
                    }
                    (DragPhase::Pressed, PressTarget::Body, OverlayMode::Expanded(_)) => {
                        next.mode = OverlayMode::Collapsed
                    }
                    (DragPhase::Pressed, PressTarget::Timeframe(t), OverlayMode::Expanded(_)) => {
                        next.settings.timeframe = *t
                    }
                    _ => {}
                }
            }
        }
        OverlayEvent::SecondaryClicked => {
            if state.drag == DragPhase::Idle {
                next.menu_open = !state.menu_open;
            }
        }
        OverlayEvent::PointerLeft => next.menu_open = false,
        OverlayEvent::ToggleLock => next.settings.locked = !state.settings.locked,
        OverlayEvent::AppActivated(app) => next.frontmost = *app,
        OverlayEvent::SettingsEdited(settings) => next.settings = settings.clone(),
    }
    if let OverlayMode::Expanded(_) = next.mode {
        next.mode = OverlayMode::Expanded(next.settings.timeframe);
    }
    if !next.is_visible() {
        next.drag = DragPhase::Idle;
        next.menu_open = false;
    }
    if next.drag == DragPhase::Dragging && (next.settings.locked || next.menu_open) {
        next.drag = DragPhase::Idle;
    }
    next
}

use crate::market::{Timeframe, ZoomStep};
use crate::settings::{ScreenPoint, Settings};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OverlayMode {
    Collapsed,
    Expanded(Timeframe),
}

impl OverlayMode {
    pub fn name(&self) -> &'static str {
        match self {
            OverlayMode::Collapsed => "collapsed",
            OverlayMode::Expanded(_) => "expanded",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Frontmost {
    Wow,
    /// This app, which is in the foreground only while its own settings window is in use.
    Ticker,
    Other,
}

impl Frontmost {
    pub const ALL: [Frontmost; 3] = [Frontmost::Wow, Frontmost::Ticker, Frontmost::Other];

    /// Retail, PTR and beta builds of the game and of Classic.
    pub const WOW_EXECUTABLES: [&str; 6] =
        ["Wow.exe", "WowT.exe", "WowB.exe", "WowClassic.exe", "WowClassicT.exe", "WowClassicB.exe"];

    /// Classifies the foreground window's process from its executable (a file name or a full path) and its pid.
    pub fn classify(foreground_executable: &str, foreground_pid: u32, our_pid: u32) -> Frontmost {
        if foreground_pid == our_pid {
            return Frontmost::Ticker;
        }
        let file_name = foreground_executable.rsplit(['\\', '/']).next().unwrap_or_default();
        if Self::WOW_EXECUTABLES.iter().any(|wow| wow.eq_ignore_ascii_case(file_name)) {
            Frontmost::Wow
        } else {
            Frontmost::Other
        }
    }
}

/// A press becomes a drag only once the pointer travels `OverlayState::DRAG_THRESHOLD` on an unlocked overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DragPhase {
    Idle,
    Pressed,
    Dragging,
}

/// What is under the pointer when the mouse button comes up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PressTarget {
    /// Anywhere on the overlay outside the timeframe controls.
    Body,
    Timeframe(Timeframe),
    Outside,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OverlayEvent {
    Pressed,
    /// The pointer is `distance` physical pixels from where the button went down.
    PointerMoved {
        distance: f64,
    },
    /// A drag drops the ticker line at `ticker_origin`. Any other press is a click: on the body it toggles between the
    /// ticker and the chart, on a timeframe control it selects that timeframe.
    Released {
        over: PressTarget,
        ticker_origin: ScreenPoint,
    },
    ToggleLock,
    /// Steps the chart one rung along the zoom ladder, and only while the chart is on screen.
    Zoom(ZoomStep),
    AppActivated(Frontmost),
    SettingsEdited(Settings),
}

/// The overlay's interaction state. The expanded chart always shows `settings.timeframe`, so the two cannot disagree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayState {
    settings: Settings,
    frontmost: Frontmost,
    expanded: bool,
    drag: DragPhase,
}

impl OverlayState {
    pub const DRAG_THRESHOLD: f64 = 3.0;

    pub fn new(settings: Settings, frontmost: Frontmost) -> Self {
        Self { settings: settings.clamped(), frontmost, expanded: false, drag: DragPhase::Idle }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn frontmost(&self) -> Frontmost {
        self.frontmost
    }

    pub fn mode(&self) -> OverlayMode {
        if self.expanded { OverlayMode::Expanded(self.settings.timeframe) } else { OverlayMode::Collapsed }
    }

    pub fn drag(&self) -> DragPhase {
        self.drag
    }

    /// The ticker counts as "WoW active" too, so the overlay stays visible while its own settings window is open.
    pub fn is_visible(&self) -> bool {
        !self.settings.only_when_wow_active || self.frontmost != Frontmost::Other
    }

    pub fn reduce(&self, event: OverlayEvent) -> OverlayState {
        let mut next = *self;
        match event {
            OverlayEvent::Pressed => next.drag = DragPhase::Pressed,
            OverlayEvent::PointerMoved { distance } => {
                if self.drag == DragPhase::Pressed && !self.settings.locked && distance >= Self::DRAG_THRESHOLD {
                    next.drag = DragPhase::Dragging;
                }
            }
            OverlayEvent::Released { over, ticker_origin } => {
                next.drag = DragPhase::Idle;
                match (self.drag, over) {
                    (DragPhase::Dragging, _) => next.settings.position = Some(ticker_origin),
                    (DragPhase::Pressed, PressTarget::Body) => next.expanded = !self.expanded,
                    (DragPhase::Pressed, PressTarget::Timeframe(timeframe)) if self.expanded => {
                        next.settings.timeframe = timeframe;
                    }
                    _ => {}
                }
            }
            OverlayEvent::ToggleLock => next.settings.locked = !self.settings.locked,
            OverlayEvent::Zoom(step) => {
                if self.expanded && self.is_visible() {
                    next.settings.zoom = self.settings.zoom.stepped(step);
                }
            }
            OverlayEvent::AppActivated(app) => next.frontmost = app,
            OverlayEvent::SettingsEdited(settings) => next.settings = settings.clamped(),
        }
        if !next.is_visible() || (next.settings.locked && next.drag == DragPhase::Dragging) {
            next.drag = DragPhase::Idle;
        }
        next
    }
}

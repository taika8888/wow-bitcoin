//! Inputs in the code's own words, so a counterexample reads as a sequence of things a person or the OS did.

use ticker_core::{Frontmost, OverlayEvent, PressTarget, Settings, ZoomStep};

pub fn overlay_event(event: &OverlayEvent) -> String {
    match event {
        OverlayEvent::Pressed => "press the mouse on the overlay".to_string(),
        OverlayEvent::PointerMoved { distance } => format!("move the pointer {distance} px from the press"),
        OverlayEvent::Released { over, ticker_origin } => {
            let target = match over {
                PressTarget::Body => "the body".to_string(),
                PressTarget::Timeframe(timeframe) => format!("the {} control", timeframe.spec().label),
                PressTarget::Outside => "outside the overlay".to_string(),
            };
            format!("release over {target} (ticker line at {}, {})", ticker_origin.x, ticker_origin.y)
        }
        OverlayEvent::ToggleLock => "toggle Lock".to_string(),
        OverlayEvent::Zoom(ZoomStep::In) => "scroll up over the chart to zoom in".to_string(),
        OverlayEvent::Zoom(ZoomStep::Out) => "scroll down over the chart to zoom out".to_string(),
        OverlayEvent::AppActivated(app) => format!("{} comes to the foreground", frontmost(*app)),
        OverlayEvent::SettingsEdited(settings) => format!("edit settings to {}", settings_words(settings)),
    }
}

pub fn frontmost(app: Frontmost) -> &'static str {
    match app {
        Frontmost::Wow => "WoW",
        Frontmost::Ticker => "the ticker's settings window",
        Frontmost::Other => "another app",
    }
}

pub fn settings_words(settings: &Settings) -> String {
    let position = settings.position.map_or("default".to_string(), |point| format!("({}, {})", point.x, point.y));
    format!(
        "locked {}, onlyWhenWoWActive {}, refresh {} s, timeframe {}, zoom {}, position {position}",
        settings.locked,
        settings.only_when_wow_active,
        settings.refresh_seconds,
        settings.timeframe.spec().label,
        settings.zoom.label()
    )
}

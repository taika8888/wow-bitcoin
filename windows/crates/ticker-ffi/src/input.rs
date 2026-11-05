use serde::Deserialize;
use serde_json::Value;
use ticker_core::{
    Frontmost, HttpOutcome, Input, OverlayEvent, PressTarget, RequestId, ScreenPoint, Settings, Timeframe, Timestamp,
    ZoomStep,
};

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Foreground {
    Wow,
    Ticker,
    Other,
}

impl From<Foreground> for Frontmost {
    fn from(value: Foreground) -> Self {
        match value {
            Foreground::Wow => Self::Wow,
            Foreground::Ticker => Self::Ticker,
            Foreground::Other => Self::Other,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    Pressed,
    PointerMoved { distance: f64 },
    Released { over: Target, x: f64, y: f64 },
    ToggleLock,
    Zoom { inward: bool },
    AppActivated { frontmost: Foreground },
    SettingsEdited { settings: Value },
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Body,
    Outside,
    M1,
    M15,
    H4,
}

impl From<Event> for OverlayEvent {
    fn from(event: Event) -> Self {
        match event {
            Event::Pressed => Self::Pressed,
            Event::PointerMoved { distance } => Self::PointerMoved { distance },
            Event::Released { over, x, y } => Self::Released {
                over: match over {
                    Target::Body => PressTarget::Body,
                    Target::Outside => PressTarget::Outside,
                    Target::M1 => PressTarget::Timeframe(Timeframe::M1),
                    Target::M15 => PressTarget::Timeframe(Timeframe::M15),
                    Target::H4 => PressTarget::Timeframe(Timeframe::H4),
                },
                ticker_origin: ScreenPoint { x, y },
            },
            Event::ToggleLock => Self::ToggleLock,
            Event::Zoom { inward } => Self::Zoom(if inward { ZoomStep::In } else { ZoomStep::Out }),
            Event::AppActivated { frontmost } => Self::AppActivated(frontmost.into()),
            Event::SettingsEdited { settings } => Self::SettingsEdited(Settings::decode(Some(&settings.to_string()))),
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    Answered { status: u16, body: String },
    Unreachable,
}

impl From<Outcome> for HttpOutcome {
    fn from(value: Outcome) -> Self {
        match value {
            Outcome::Answered { status, body } => Self::Answered { status, body: body.into_bytes() },
            Outcome::Unreachable => Self::Unreachable,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Message {
    Overlay { event: Event },
    Tick { now: f64 },
    Woke { now: f64 },
    SettingsChanged { settings: Value },
    QuoteFinished { request: u64, outcome: Outcome, now: f64 },
    CandlesFinished { request: u64, timeframe: Timeframe, outcome: Outcome, now: f64 },
}

impl From<Message> for Input {
    fn from(message: Message) -> Self {
        match message {
            Message::Overlay { event } => Self::Overlay(event.into()),
            Message::Tick { now } => Self::Tick(Timestamp::from_unix_seconds(now)),
            Message::Woke { now } => Self::Woke(Timestamp::from_unix_seconds(now)),
            Message::SettingsChanged { settings } => {
                Self::SettingsChanged(Settings::decode(Some(&settings.to_string())))
            }
            Message::QuoteFinished { request, outcome, now } => Self::QuoteFinished {
                request: RequestId::from_raw(request),
                outcome: outcome.into(),
                now: Timestamp::from_unix_seconds(now),
            },
            Message::CandlesFinished { request, timeframe, outcome, now } => Self::CandlesFinished {
                request: RequestId::from_raw(request),
                timeframe,
                outcome: outcome.into(),
                now: Timestamp::from_unix_seconds(now),
            },
        }
    }
}

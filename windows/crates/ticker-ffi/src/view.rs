use serde::Serialize;
use ticker_core::{App, CandleChart, ChartNotice, DragPhase, Effect, Settings, TickerDisplay, Timeframe, Tone};

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    SaveSettings { settings: Settings },
    FetchQuote { request: u64, url: &'static str, timeout: f64 },
    FetchCandles { request: u64, timeframe: Timeframe, url: String, timeout: f64 },
    Render,
}

impl From<Effect> for Action {
    fn from(effect: Effect) -> Self {
        match effect {
            Effect::SaveSettings(settings) => Self::SaveSettings { settings },
            Effect::FetchQuote { request, url, timeout } => {
                Self::FetchQuote { request: request.raw(), url, timeout: timeout.as_secs_f64() }
            }
            Effect::FetchCandles { request, timeframe, url, timeout } => {
                Self::FetchCandles { request: request.raw(), timeframe, url, timeout: timeout.as_secs_f64() }
            }
            Effect::Render => Self::Render,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    pub effects: Vec<Action>,
    pub next_wake: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    settings: Settings,
    overlay: Overlay,
    status: Status,
    display: Display,
    load: Option<Load>,
    chart_revision: u64,
    chart_start: Option<f64>,
    chart: Option<Chart>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Overlay {
    mode: &'static str,
    timeframe: Option<Timeframe>,
    drag: &'static str,
    is_visible: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    name: &'static str,
    fetched_at: Option<f64>,
}

#[derive(Serialize)]
struct Display {
    segments: Vec<Segment>,
    dimmed: bool,
}

#[derive(Serialize)]
struct Segment {
    text: String,
    tone: Tone,
}

#[derive(Serialize)]
struct Load {
    name: &'static str,
    notice: Option<Notice>,
}

#[derive(Serialize)]
struct Notice {
    kind: &'static str,
    text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Chart {
    bars: Vec<Bar>,
    price_domain: [f64; 2],
    time_domain: [f64; 2],
    last_price: f64,
    candle_count: usize,
    price_ticks: Vec<PriceTick>,
    time_ticks: Vec<f64>,
}

#[derive(Serialize)]
struct PriceTick {
    value: f64,
    label: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Bar {
    time: f64,
    low: f64,
    high: f64,
    body_low: f64,
    body_high: f64,
    tone: Tone,
}

impl View {
    pub fn new(app: &App, revision: u64, max_bars: usize, utc_offset: i32) -> Self {
        let overlay = app.overlay();
        let status = app.status();
        let display = app.display();
        let expanded = app.expanded();
        // Metadata reads never merge/serialize bars. The shell requests them only when the chart cache key changes.
        let chart = (max_bars > 0).then(|| app.chart(max_bars)).flatten();
        let chart_start = expanded.and_then(|(timeframe, load)| {
            let candles = &load.series()?.candles;
            let count = app.settings().zoom.visible_count(timeframe, candles.len());
            candles.get(candles.len().checked_sub(count)?).map(|candle| candle.time.unix_seconds())
        });
        Self {
            settings: *app.settings(),
            overlay: Overlay {
                mode: overlay.mode().name(),
                timeframe: expanded.map(|(timeframe, _)| timeframe),
                drag: match overlay.drag() {
                    DragPhase::Idle => "idle",
                    DragPhase::Pressed => "pressed",
                    DragPhase::Dragging => "dragging",
                },
                is_visible: overlay.is_visible(),
            },
            status: Status {
                name: status.name(),
                fetched_at: status.quote().map(|quote| quote.fetched_at.unix_seconds()),
            },
            display: Display {
                segments: display
                    .segments
                    .into_iter()
                    .map(|segment| Segment { text: segment.text, tone: segment.tone })
                    .collect(),
                dimmed: display.dimmed,
            },
            load: expanded.map(|(timeframe, load)| Load {
                name: load.name(),
                notice: load.notice(timeframe).map(|notice| match notice {
                    ChartNotice::Placeholder(text) => Notice { kind: "placeholder", text },
                    ChartNotice::Stale(text) => Notice { kind: "stale", text },
                }),
            }),
            chart_revision: revision,
            chart_start,
            chart: chart.map(|chart| Chart::new(chart, utc_offset)),
        }
    }
}

impl Chart {
    fn new(chart: CandleChart, utc_offset: i32) -> Self {
        Self {
            price_ticks: chart
                .price_ticks()
                .into_iter()
                .map(|value| PriceTick { value, label: TickerDisplay::price(value, 0) })
                .collect(),
            time_ticks: chart.time_ticks(utc_offset).into_iter().map(|time| time.unix_seconds()).collect(),
            price_domain: [*chart.price_domain.start(), *chart.price_domain.end()],
            time_domain: [chart.time_domain.start().unix_seconds(), chart.time_domain.end().unix_seconds()],
            last_price: chart.last_price,
            candle_count: chart.candle_count,
            bars: chart
                .bars
                .into_iter()
                .map(|bar| Bar {
                    time: bar.time.unix_seconds(),
                    low: bar.low,
                    high: bar.high,
                    body_low: bar.body_low,
                    body_high: bar.body_high,
                    tone: bar.tone,
                })
                .collect(),
        }
    }
}
